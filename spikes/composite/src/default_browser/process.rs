//! Bounded subprocesses with file-backed output: inherited pipes cannot hang
//! the host after a timed-out desktop helper exits. No assembled shell command.
use std::{io::{Read,Seek,SeekFrom},path::Path,process::{Command,Stdio},time::{Duration,Instant}};
pub fn run(program:&Path,args:&[&str],timeout:Duration)->Result<Vec<u8>,String> {
    let mut output=tempfile::tempfile().map_err(|_|"Could not create a temporary query buffer.")?;
    let error=tempfile::tempfile().map_err(|_|"Could not create a temporary error buffer.")?;
    let mut cmd=Command::new(program);
    cmd.args(args).stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|_|"Could not open the query buffer.")?)
        .stderr(error);
    for k in ["NUS_SHOT","NUS_SHOT2","NUS_SHOT_DIR","NUS_SHOT_OUT","NUS_SHOT_SIZE","NUS_PERF"] {cmd.env_remove(k);}
    #[cfg(unix)] {use std::os::unix::process::CommandExt;cmd.process_group(0);}
    #[cfg(windows)] {use std::os::windows::process::CommandExt;cmd.creation_flags(0x08000000);}
    let mut child=cmd.spawn().map_err(|_|"The operating-system integration helper is unavailable.")?;
    let until=Instant::now()+timeout;
    let status=loop {
        match child.try_wait() {
            Ok(Some(status))=>break status,
            Ok(None) if Instant::now()<until=>std::thread::sleep(Duration::from_millis(20)),
            _=>{
                #[cfg(unix)] unsafe {libc::kill(-(child.id()as i32),libc::SIGKILL);}
                let _=child.kill();let _=child.wait();
                return Err("The operating-system integration helper did not answer in time.".into());
            }
        }
    };
    if !status.success() {return Err(format!("The operating-system integration helper failed (code {:?}).",status.code()));}
    output.seek(SeekFrom::Start(0)).map_err(|_|"Could not read the query result.")?;
    let mut bytes=Vec::new();output.take(16385).read_to_end(&mut bytes).map_err(|_|"Could not read the query result.")?;
    if bytes.len()>16384 {return Err("The query result exceeded its size limit.".into());}
    Ok(bytes)
}
