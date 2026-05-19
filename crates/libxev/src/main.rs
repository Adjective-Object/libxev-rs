use libxev::{Loop, RunMode};

fn main() -> std::io::Result<()> {
    let mut ev = Loop::new()?;
    // Nothing scheduled — should return immediately.
    ev.run(RunMode::NoWait)?;
    println!("libxev loop ran with no pending work");
    Ok(())
}
