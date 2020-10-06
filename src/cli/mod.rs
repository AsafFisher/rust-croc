use clap::Clap;
use std::path::Path;
use std::io::{self, Write};
use std::option::Option;
#[derive(Debug, Clap)]
#[clap(
    name = concat!(
        "NAME: \n   ",
        env!("CARGO_BIN_NAME"),
        " - easily and securely transfer stuff from one computer to another\n\n",
        "VERSION: \n   ",
        concat!("v", env!("CARGO_PKG_VERSION"), "-", env!("CURRENT_COMMIT_HASH"))
        
    ),
)]
struct Opts {
    /// automatically agree to all prompts (default: false)
    #[clap(long, short)]
    yes: bool,
    #[clap(subcommand)]
    subcmd: Option<Commands>,
}

#[derive(Debug, Clap)]
enum Commands {
    Send(Send),
}

#[derive(Debug, Clap)]
/// send a file (see options with croc send -h)
struct Send {
    #[clap(short, long, value_name = "value")]
    /// codephrase used to connect to relay
    code: Option<String>,

    #[clap(long)]
    /// disable local relay when sending
    no_local: bool,

    #[clap(long)]
    /// disable multiplexing
    no_multi: bool,

    #[clap(long)]
    /// ports of the local relay (optional) (default: "9009,9010,9011,9012,9013")
    port: Option<String>,

    #[clap(required = true, validator=file_exist)]
    /// send a file/files over the relay
    file_name: Vec<String>,
}
fn file_exist(val: &str) -> Result<(), io::Error> {
    if !Path::new(val).exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "File  Not Found!"));
    }
    Ok(())
}
pub fn run() -> Result<(), io::Error>{
    let opts: Opts = Opts::parse();
    match opts.subcmd {
        None => {
            return run_croc();
        },
        Some(cmd) => {
            println!("{:?}", cmd);
            Ok(())
        }
    }
}

fn run_croc() -> Result<(), io::Error>{
    let mut input = String::new();
    print!("Enter receive code: ");
    io::stdout().flush().unwrap();
    match io::stdin().read_line(& mut input){
        Ok(_) => (),
        Err(error) => {
            //panic!("{:?}", error);
            return Err(error)
        }
    };
    println!("{:?}", input.trim());
    Ok(())
}