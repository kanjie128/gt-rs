#![feature(naked_functions)]
use core::arch::asm;

const STACK_SIZE: usize = 1024 * 1024 * 4;
const THREAD_SIZE: usize = 4;
static mut RUNTIME: usize = 0;

struct RunTime {
    threads: Vec<Thread>,
    current: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum ThreadState {
    Available,
    Running,
    Ready,
}

struct Thread {
    id: usize,
    // stack should not move to other memory address
    stack: *mut [u8],
    ctx: ThreadContext,
    state: ThreadState,
}

impl Drop for Thread {
    fn drop(&mut self) {
        unsafe {
            // convert stack back to Box, then drop
            Box::from_raw(self.stack);
            println!("thread {} exit", self.id);
        }
    }
}

// callee saved register should store carefully
#[repr(C)]
#[derive(Default, Debug)]
struct ThreadContext {
    rsp: usize,
    r15: usize,
    r14: usize,
    r13: usize,
    r12: usize,
    rbx: usize,
    rbp: usize,
}

impl Thread {
    fn new(id: usize) -> Self {
        let buff = Box::new([0u8; STACK_SIZE]);
        // manage stack memory our self
        let stack = Box::into_raw(buff);
        Self {
            id,
            ctx: Default::default(),
            state: ThreadState::Available,
            stack,
        }
    }
}

impl RunTime {
    fn new() -> Self {
        let mut threads = (0..THREAD_SIZE).map(Thread::new).collect::<Vec<_>>();
        // mark thread 0 as base thread
        threads[0].state = ThreadState::Running;
        Self {
            current: 0,
            threads,
        }
    }

    // init runtime as global variable for convenient use
    fn init(&self) {
        unsafe {
            RUNTIME = self as *const _ as usize;
        }
    }

    // spawn_task take task to run with RunTime threads
    fn spawn_task(&mut self, task: fn()) {
        // find available thread
        let available_thread = self
            .threads
            .iter_mut()
            .find(|t| t.state == ThreadState::Available)
            .expect("no available thread to run task");
        // set up task on available thread
        unsafe {
            let stack = (&mut (*available_thread.stack)[0]) as *mut u8;
            // stack address align to 16 bytes
            let stack_bottom = (stack.add(STACK_SIZE) as usize & !0xFF) as *mut u8;
            std::ptr::write(stack_bottom.offset(-16) as *mut usize, task_return as usize);
            std::ptr::write(stack_bottom.offset(-24) as *mut usize, just_ret as usize);
            std::ptr::write(stack_bottom.offset(-32) as *mut usize, task as usize);
            available_thread.ctx.rsp = stack_bottom.offset(-32) as usize;
        }
        available_thread.state = ThreadState::Ready;
    }

    fn yield_out(&mut self) -> bool {
        let mut pos = self.current;
        while self.threads[pos].state != ThreadState::Ready {
            pos += 1;
            if pos == THREAD_SIZE {
                pos = 0;
            }
            if pos == self.current {
                return false;
            }
        }
        if self.threads[self.current].state != ThreadState::Available {
            self.threads[self.current].state = ThreadState::Ready;
        }
        let old = self.current;
        self.current = pos;
        println!("thread: {} start yield", old);
        println!("thread: {} start running", pos);
        self.threads[self.current].state = ThreadState::Running;
        unsafe {
            let old_ctx: *mut ThreadContext = &mut self.threads[old].ctx;
            let new_ctx: *const ThreadContext = &self.threads[pos].ctx;
            // 1. call function `ctx_switch` directly so `ctx_switch` should not mangle.
            // 2. by linux call convention, `rdi` store the first parameter of
            //    the calling function, `rsi` store the second. see more detail:
            //    https://en.wikipedia.org/wiki/X86_calling_conventions#List_of_x86_calling_conventions
            // 3. clobber_abi("C") tells the compiler to push the values of these
            //    registers on to the stack before calling ctx_switch and pop
            //    them back in to the same registers once the function returns.
            asm!("call ctx_switch", in("rdi") old_ctx, in("rsi" ) new_ctx, clobber_abi("C"));
        }
        true
    }

    fn run(&mut self) {
        while self.yield_out() {}
        println!("no available task to run, run time exit");
        // std::process::exit(0);
    }

    fn ret(&mut self) {
        if self.current != 0 {
            self.threads[self.current].state = ThreadState::Available;
            self.yield_out();
        }
    }
}

// return to next return address
#[naked]
unsafe extern "C" fn just_ret() {
    asm!("ret", options(noreturn))
}

// callee saved registers should be save to thread local variables
// and restore new thread context to run.
#[naked]
#[no_mangle]
extern "C" fn ctx_switch() {
    unsafe {
        asm!(
            "mov [rdi + 0x00], rsp",
            "mov [rdi + 0x08], r15",
            "mov [rdi + 0x10], r14",
            "mov [rdi + 0x18], r13",
            "mov [rdi + 0x20], r12",
            "mov [rdi + 0x28], rbx",
            "mov [rdi + 0x30], rbp",
            "mov rsp, [rsi + 0x00]",
            "mov r15, [rsi + 0x08]",
            "mov r14, [rsi + 0x10]",
            "mov r13, [rsi + 0x18]",
            "mov r12, [rsi + 0x20]",
            "mov rbx, [rsi + 0x28]",
            "mov rbp, [rsi + 0x30]",
            "ret",
            options(noreturn)
        );
    }
}

// when task return, we should do thread yield
pub fn task_return() {
    unsafe {
        let rt_ptr = RUNTIME as *mut RunTime;
        (*rt_ptr).ret();
    }
}

// tasks may call this function to yield out themselves
pub fn yield_thread() {
    unsafe {
        let rt_ptr = RUNTIME as *mut RunTime;
        (*rt_ptr).yield_out();
    };
}

fn main() {
    let mut rt = RunTime::new();
    rt.init();
    rt.spawn_task(|| {
        let task_id = 1;
        for i in 0..10 {
            println!("in task: {}, conter: {}", task_id, i);
            yield_thread();
        }
        println!("in task: {}, finished", task_id);
    });
    rt.spawn_task(|| {
        let task_id = 2;
        for i in 0..10 {
            println!("in task: {}, conter: {}", task_id, i);
            yield_thread();
        }
        println!("in task: {}, finished", task_id);
    });
    rt.run();
}
