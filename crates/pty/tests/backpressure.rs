//! A noisy process pauses while its consumer is busy and resumes without loss.
#[cfg(unix)]
#[test]
fn sustained_output_is_bounded_per_drain_and_lossless() {
    use nus_pty::{Profile,Pty};
    use std::time::{Duration,Instant};
    let profile=Profile{name:"noisy".into(),program:"sh".into(),args:vec!["-c".into(),"head -c 8388608 /dev/zero | tr '\\000' x".into()],cwd:None,env:vec![]};
    let pty=Pty::spawn(&profile,80,24,||{}).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    let mut count=0;let deadline=Instant::now()+Duration::from_secs(15);
    while count<8388608 && Instant::now()<deadline {
        let bytes=pty.take_output();
        assert!(bytes.len()<=1024*1024+64*1024);
        count+=bytes.iter().filter(|b|**b==b'x').count();
        if bytes.is_empty(){std::thread::sleep(Duration::from_millis(2));}
    }
    assert_eq!(count,8388608,"queued output was lost or never resumed");
}
