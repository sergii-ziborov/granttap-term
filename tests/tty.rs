use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn granttap() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_granttap-term"));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn piped_stdio_is_not_a_terminal() {
    let mut child = granttap().spawn().expect("spawn");
    drop(child.stdin.take());
    let mut err = String::new();
    child
        .stderr
        .take()
        .expect("stderr")
        .read_to_string(&mut err)
        .expect("read stderr");
    let status = child.wait().expect("wait");
    assert!(!status.success(), "{status}");
    assert!(
        err.contains("granttap-term: run this inside a terminal"),
        "{err}"
    );
}

#[test]
fn mcp_initialize_replies_on_stdout() {
    let mut child = granttap().arg("mcp").spawn().expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"initialize\"}\n")
            .expect("write stdin");
    }
    let mut out = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut out)
        .expect("read stdout");
    let status = child.wait().expect("wait");
    assert!(status.success(), "{status}");
    assert!(out.contains("granttap-local"), "{out}");
    assert!(out.contains("1.0.0"), "{out}");
    assert!(out.contains("\"id\":7"), "{out}");
}
