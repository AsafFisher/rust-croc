use std::error::Error;
use std::process::Command;
fn main() -> Result<(), Box<dyn Error>>{    
    let output = match Command::new("git").args(&["log","--pretty=format:%h","-n1"]).output() {
        Ok(out) => out,
        Err(err) => {
            panic!("{:?}", err);
        }
    };
    let current_commit_hash = match String::from_utf8(output.stdout){
        Ok(out) => out,
        Err(err) => {
            panic!("{:?}",err);
        }
    };
    println!("cargo:rustc-env=CURRENT_COMMIT_HASH={}", current_commit_hash);
    Ok(())
}
