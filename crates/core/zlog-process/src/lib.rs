use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub memory: u64,
    pub timeout: Duration,
    pub greeting: String,
}

impl Spec {
    pub fn subcommand(name: &str) -> io::Result<Spec> {
        Ok(Spec {
            program: std::env::current_exe()?,
            args: vec![name.to_string()],
            memory: 128 << 20,
            timeout: Duration::from_secs(10),
            greeting: String::new(),
        })
    }
}

static RUNNING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

static EXTERNAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn external_spawns() -> usize {
    EXTERNAL.load(std::sync::atomic::Ordering::SeqCst)
}

fn is_external(program: &std::path::Path) -> bool {
    !std::env::current_exe().is_ok_and(|exe| exe == program)
}

fn count_external(program: &std::path::Path) {
    if is_external(program) {
        EXTERNAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn external(program: impl AsRef<std::ffi::OsStr>) -> Command {
    count_external(std::path::Path::new(program.as_ref()));
    Command::new(program)
}

pub fn running() -> Vec<u32> {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

type Started = io::Result<(Child, limits::Job)>;

type Spawn = (Command, u64, Sender<Started>);

static EXITING: AtomicBool = AtomicBool::new(false);

static SPAWNING: Mutex<()> = Mutex::new(());

const EXITING_ERROR: &str = "zLog is quitting";

pub fn shut_down() {
    EXITING.store(true, Ordering::SeqCst);
    drop(SPAWNING.lock().unwrap_or_else(|e| e.into_inner()));
}

pub fn exiting() -> bool {
    EXITING.load(Ordering::SeqCst)
}

fn kill(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn start(mut command: Command, memory: u64) -> Started {
    let _busy = SPAWNING.lock().unwrap_or_else(|e| e.into_inner());
    if exiting() {
        return Err(io::Error::other(EXITING_ERROR));
    }
    let mut child = command.spawn()?;
    let job = match limits::after_start(&child, memory) {
        Ok(job) if !exiting() => job,
        Ok(_) => {
            kill(&mut child);
            return Err(io::Error::other(EXITING_ERROR));
        }
        Err(e) => {
            kill(&mut child);
            return Err(e);
        }
    };
    Ok((child, job))
}

fn spawn_on_long_lived_thread(command: Command, memory: u64) -> Started {
    if exiting() {
        return Err(io::Error::other(EXITING_ERROR));
    }
    static SPAWNER: OnceLock<Mutex<Sender<Spawn>>> = OnceLock::new();
    let spawner = SPAWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Spawn>();
        std::thread::Builder::new()
            .name("zlog-process-spawner".into())
            .spawn(move || {
                for (command, memory, reply) in rx {
                    if let Err(mpsc::SendError(Ok((mut child, _job)))) =
                        reply.send(start(command, memory))
                    {
                        kill(&mut child);
                    }
                }
            })
            .expect("the spawner thread starts");
        Mutex::new(tx)
    });
    let (tx, rx) = mpsc::channel();
    spawner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send((command, memory, tx))
        .map_err(|_| io::Error::other("the spawner thread stopped"))?;
    rx.recv()
        .map_err(|_| io::Error::other("the spawner thread stopped"))?
}

pub struct Process {
    child: Child,
    id: u32,
    _job: limits::Job,
}

impl Process {
    pub fn spawn(spec: &Spec, stderr: Stdio) -> io::Result<(Process, ChildStdin, ChildStdout)> {
        count_external(&spec.program);
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr);
        limits::before_start(&mut command, spec.memory);
        let (mut child, job) = spawn_on_long_lived_thread(command, spec.memory)?;
        let id = child.id();
        let pipes = child.stdin.take().zip(child.stdout.take());
        let process = Process {
            child,
            id,
            _job: job,
        };
        let (stdin, stdout) = pipes.ok_or(io::ErrorKind::BrokenPipe)?;
        RUNNING.lock().unwrap_or_else(|e| e.into_inner()).push(id);
        Ok((process, stdin, stdout))
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn signal_kill(&mut self) {
        let _ = self.child.kill();
    }

    pub fn kill(&mut self) {
        kill(&mut self.child);
        RUNNING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|c| *c != self.id);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Watch {
    child: Mutex<Process>,
    base: Instant,
    busy: AtomicU64,
    stop: AtomicBool,
    timeout: Duration,
}

impl Watch {
    fn now(&self) -> u64 {
        self.base.elapsed().as_millis() as u64 + 1
    }

    fn guard(self: Arc<Self>) {
        while !self.stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
            let since = self.busy.load(Ordering::SeqCst);
            if since != 0 && self.now() - since > self.timeout.as_millis() as u64 {
                self.child
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .signal_kill();
            }
        }
    }
}

pub struct Helper {
    watch: Arc<Watch>,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u32,
}

impl Helper {
    pub fn start(spec: &Spec) -> io::Result<Helper> {
        let (child, stdin, stdout) = Process::spawn(spec, Stdio::null())?;
        let id = child.id();
        let stdout = BufReader::new(stdout);
        let watch = Arc::new(Watch {
            child: Mutex::new(child),
            base: Instant::now(),
            busy: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            timeout: spec.timeout,
        });
        let guard = watch.clone();
        std::thread::spawn(move || guard.guard());
        let mut helper = Helper {
            watch,
            stdin,
            stdout,
            id,
        };
        helper
            .watch
            .busy
            .store(helper.watch.now(), Ordering::SeqCst);
        loop {
            let line = helper.read();
            if line?.ends_with(&spec.greeting) {
                helper.watch.busy.store(0, Ordering::SeqCst);
                return Ok(helper);
            }
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    fn line(&mut self) -> io::Result<String> {
        self.watch.busy.store(self.watch.now(), Ordering::SeqCst);
        let line = self.read();
        self.watch.busy.store(0, Ordering::SeqCst);
        line
    }

    fn read(&mut self) -> io::Result<String> {
        let mut line = String::new();
        match self.stdout.read_line(&mut line)? {
            0 => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the helper process stopped or did not answer in time",
            )),
            _ => Ok(line.trim_end_matches(['\r', '\n']).to_string()),
        }
    }

    pub fn request(&mut self, line: &str) -> io::Result<String> {
        writeln!(self.stdin, "{line}")?;
        self.stdin.flush()?;
        self.line()
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        self.watch.stop.store(true, Ordering::SeqCst);
        self.watch
            .child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .kill();
    }
}

pub const HEALTHY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Restarts {
    limit: u32,
    failures: u32,
    since: Option<Instant>,
}

impl Restarts {
    pub fn new(limit: u32) -> Self {
        Restarts {
            limit,
            failures: 0,
            since: None,
        }
    }

    pub fn started(&mut self) {
        self.since = Some(Instant::now());
    }

    pub fn failed(&mut self) -> bool {
        let up = self.since.take().map(|s| s.elapsed()).unwrap_or_default();
        self.failed_after(up)
    }

    pub fn failed_after(&mut self, up: Duration) -> bool {
        if up >= HEALTHY {
            self.failures = 0;
        }
        self.failures += 1;
        self.failures <= self.limit
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }
}

#[cfg(unix)]
mod limits {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    pub struct Job;

    pub fn before_start(command: &mut Command, memory: u64) {
        let limit = libc::rlimit {
            rlim_cur: memory as libc::rlim_t,
            rlim_max: memory as libc::rlim_t,
        };
        #[cfg(target_os = "linux")]
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: the closure runs in the forked child before exec and only calls
        // async-signal-safe functions (setrlimit, prctl, getppid) on plain data copied into it.
        unsafe {
            command.pre_exec(move || {
                if memory != 0 {
                    libc::setrlimit(libc::RLIMIT_DATA, &limit);
                }
                #[cfg(target_os = "linux")]
                {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                    if libc::getppid() != parent {
                        return Err(std::io::ErrorKind::BrokenPipe.into());
                    }
                }
                Ok(())
            });
        }
    }

    pub fn after_start(_child: &Child, _memory: u64) -> std::io::Result<Job> {
        Ok(Job)
    }
}

#[cfg(windows)]
mod limits {
    use std::io;
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    };
    use windows_sys::Win32::System::Threading::{
        OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
    };

    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle is valid and owned, and is closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    pub struct Job {
        _handle: Option<Handle>,
    }

    // SAFETY: a job handle may be used and closed from any thread.
    unsafe impl Send for Job {}

    pub fn before_start(command: &mut Command, _memory: u64) {
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
    }

    fn resume(pid: u32) -> io::Result<()> {
        // SAFETY: every handle is checked and closed by `Handle`, and the entry struct is
        // plain data whose size field is set before the first call.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let snapshot = Handle(snapshot);
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut resumed = 0;
            let mut more = Thread32First(snapshot.0, &mut entry) != 0;
            while more {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let thread = Handle(thread);
                    if ResumeThread(thread.0) == u32::MAX {
                        return Err(io::Error::last_os_error());
                    }
                    resumed += 1;
                }
                more = Thread32Next(snapshot.0, &mut entry) != 0;
            }
            match resumed {
                0 => Err(io::Error::other(
                    "the helper process has no thread to resume",
                )),
                _ => Ok(()),
            }
        }
    }

    pub fn after_start(child: &Child, memory: u64) -> io::Result<Job> {
        // SAFETY: the job handle is checked before use, the limit struct is plain data
        // of the size passed, and the child handle stays valid while `child` lives.
        let job = unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Handle(job);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = match memory {
                0 => JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                _ => JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            };
            info.ProcessMemoryLimit = memory as usize;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() != Some(ERROR_ACCESS_DENIED as i32) {
                    return Err(e);
                }
                eprintln!(
                    "zlog-process: helper {} runs outside a job ({e}); it has no memory limit \
and stops when zLog closes its pipes",
                    child.id()
                );
                Job { _handle: None }
            } else {
                Job { _handle: Some(job) }
            }
        };
        resume(child.id())?;
        Ok(job)
    }
}

#[cfg(not(any(unix, windows)))]
mod limits {
    use std::process::{Child, Command};

    pub struct Job;

    pub fn before_start(_command: &mut Command, _memory: u64) {}

    pub fn after_start(_child: &Child, _memory: u64) -> std::io::Result<Job> {
        Ok(Job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn shell(script: &str, timeout: Duration) -> Spec {
        Spec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            memory: 256 << 20,
            timeout,
            greeting: "hello".into(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_answers_line_by_line_and_dies_with_its_handle() {
        let mut h = Helper::start(&shell(
            "echo noise; echo hello; while read l; do echo \"got $l\"; done",
            Duration::from_secs(5),
        ))
        .unwrap();
        assert_eq!(h.request("a").unwrap(), "got a");
        assert_eq!(h.request("b").unwrap(), "got b");
        let id = h.id();
        assert!(running().contains(&id));
        drop(h);
        assert!(!running().contains(&id));
    }

    #[cfg(unix)]
    #[test]
    fn a_silent_helper_is_killed_after_the_time_limit() {
        let mut h = Helper::start(&shell(
            "echo hello; read l; exec sleep 30",
            Duration::from_millis(300),
        ))
        .unwrap();
        let t = Instant::now();
        assert!(h.request("x").is_err());
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_program_or_greeting_is_an_error() {
        let mut s = shell("exit 0", Duration::from_secs(5));
        assert!(Helper::start(&s).is_err());
        s.program = "/nonexistent/zlog".into();
        assert!(Helper::start(&s).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_raw_process_has_its_pipes_no_memory_limit_at_zero_and_dies_when_dropped() {
        use std::io::Read;
        let mut s = shell("ulimit -d; read l; echo \"got $l\"", Duration::from_secs(5));
        s.memory = 0;
        let (p, mut stdin, mut stdout) = Process::spawn(&s, Stdio::null()).unwrap();
        let id = p.id();
        assert!(running().contains(&id));
        writeln!(stdin, "x").unwrap();
        let mut out = String::new();
        stdout.read_to_string(&mut out).unwrap();
        assert_eq!(out, "unlimited\ngot x\n");
        let (p, _stdin, _stdout) = Process::spawn(
            &shell("exec sleep 30", Duration::from_secs(5)),
            Stdio::null(),
        )
        .unwrap();
        let id2 = p.id();
        let t = Instant::now();
        drop(p);
        assert!(t.elapsed() < Duration::from_secs(5));
        assert!(!running().contains(&id2));
    }

    #[test]
    fn a_limit_of_three_allows_three_restarts_and_a_healthy_run_resets_it() {
        let mut r = Restarts::new(3);
        assert!(r.failed());
        assert!(r.failed());
        assert!(r.failed());
        assert!(!r.failed());
        assert_eq!(r.failures(), 4);
        let mut r = Restarts::new(3);
        for _ in 0..10 {
            assert!(r.failed_after(HEALTHY));
        }
        assert!(r.failed_after(Duration::from_secs(1)));
        assert!(r.failed_after(Duration::from_secs(1)));
        assert!(!r.failed_after(Duration::from_secs(1)));
        r.started();
        assert!(!r.failed());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_foreign_programs_count_as_external() {
        assert!(!is_external(&std::env::current_exe().unwrap()));
        assert!(is_external(std::path::Path::new("true")));
        let before = external_spawns();
        let _ = external("true");
        assert!(external_spawns() > before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_outlives_the_thread_that_spawned_it() {
        let (p, _stdin, _stdout) = std::thread::spawn(|| {
            Process::spawn(
                &shell("exec sleep 30", Duration::from_secs(5)),
                Stdio::null(),
            )
            .unwrap()
        })
        .join()
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let alive = unsafe { libc::kill(p.id() as libc::pid_t, 0) } == 0;
        let status = std::fs::read_to_string(format!("/proc/{}/stat", p.id())).unwrap_or_default();
        assert!(alive && !status.contains(") Z "), "{status}");
    }

    #[cfg(unix)]
    #[test]
    fn a_chatty_helper_without_a_greeting_times_out() {
        let t = Instant::now();
        let s = shell(
            "while true; do echo noise; sleep 0.05; done",
            Duration::from_millis(400),
        );
        assert!(Helper::start(&s).is_err());
        assert!(t.elapsed() < Duration::from_secs(5));
    }
}
