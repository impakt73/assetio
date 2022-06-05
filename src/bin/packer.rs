use assetio::{AssetDescription, Builder};
use std::fs::File;
use walkdir::WalkDir;

use clap::Parser;

/// Asset Library Packer
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path of the directory to pack
    directory: String,

    /// Name of the library output file
    output: String,
}

pub fn main() {
    let args = Args::parse();

    let mut builder = Builder::new();

    for entry in WalkDir::new(&args.directory)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.metadata().unwrap().is_file() {
            // TODO: Make this work with relative paths instead of absolute (You'll need to add a base path to the builder)
            let path = entry.path().to_str().unwrap().replace('\\', "/");
            let desc = AssetDescription::new(&path);

            println!("Found Asset: {}", &path);

            builder.insert(&desc);
        }
    }

    match File::create(&args.output) {
        Ok(mut output) => match builder.build(&mut output) {
            Ok(_) => {
                println!("Successfully wrote asset library to {}", &args.output);
            }
            Err(err) => {
                eprintln!("Error building asset library: [{}]", err);
            }
        },
        Err(err) => {
            eprintln!(
                "Unable to open output file at path: {} [{}]",
                &args.output, err
            );
        }
    }
}
