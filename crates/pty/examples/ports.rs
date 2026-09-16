fn main() {
    for p in nus_pty::listening_ports() {
        println!("{:>5}  {:>6}  {}", p.port, p.pid, p.process);
    }
}
