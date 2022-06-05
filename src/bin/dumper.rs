use assetio::Library;
use std::fs::File;

use clap::Parser;

/// Asset Library Dumper
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Path of the library to dump
    path: String,
}

pub fn main() {
    let args = Args::parse();

    let file = File::open(args.path).unwrap();
    let library = Library::new(&file).unwrap();
    for asset in library.assets() {
        println!(
            "Found Asset: [Id: {:#x}, Size: {}]",
            asset.id.raw(),
            asset.size
        );
    }
}
