fn main() {
    let ws = winsrv::WindowServer::connect().expect("windowserver");
    for s in ws.spaces() {
        println!(
            "space id={} current={} fullscreen={}",
            s.id, s.current, s.fullscreen
        );
    }
}
