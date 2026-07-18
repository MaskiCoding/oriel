fn main() {
    let caps = winsrv::probe();
    match caps.connection {
        Some(cid) => println!("skylight: connection {cid}"),
        None => println!("skylight: unavailable"),
    }
    if !caps.missing.is_empty() {
        println!("skylight: missing symbols: {}", caps.missing.join(", "));
    }
}
