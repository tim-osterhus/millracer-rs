fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") | Some("-V") | Some("version") => {
            println!("millracer {}", env!("CARGO_PKG_VERSION"));
        }
        Some("--help") | Some("-h") | Some("help") | None => {
            print_help();
        }
        Some(command) => {
            eprintln!("millracer: unknown command `{command}`");
            eprintln!("Run `millracer --help` for usage.");
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!(
        "\
Millracer {}

Rust implementation seed for the Millracer operator harness.

USAGE:
    millracer [COMMAND]

COMMANDS:
    help       Show this help text
    version    Print the installed version

This crate currently claims the public Rust package name. The autonomous
Millrace porting harness in this repository will fill in parity with the
Python Millracer reference implementation.",
        env!("CARGO_PKG_VERSION")
    );
}
