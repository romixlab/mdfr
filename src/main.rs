extern crate clap;
extern crate nom;

use clap::{Arg, App, SubCommand};
use std::fs::File;
use std::io::BufReader;
use std::io;
mod mdfinfo;

fn main() -> io::Result<()>{
    let matches = App::new("mdfr")
                          .version("0.1.0")
                          .author("Aymeric Rateau <aymeric.rateau@gmail.com>")
                          .about("reads ASAM mdf file")
                          .arg(Arg::with_name("file")
                               .help("Sets the input file to use")
                               .required(true)
                               .index(1))
                          .arg(Arg::with_name("v")
                               .short("v")
                               .multiple(true)
                               .help("Sets the level of verbosity"))
                          .subcommand(SubCommand::with_name("test")
                                      .about("controls testing features")
                                      .version("0.1")
                                      .author("Aymeric Rateau <aymeric.rateau@gmail.com>")
                                      .arg(Arg::with_name("debug")
                                          .short("d")
                                          .help("print debug information verbosely")))
                          .get_matches();

    let file_name = matches.value_of("file")
        .expect("File name missing");

    if let Some(matches) = matches.subcommand_matches("test") {
        if matches.is_present("debug") {
            println!("Printing debug info...");
        } else {
            println!("Printing normally...");
        }
    }

    let f = File::open(file_name).expect("Cannot find the file");
    let mut rdr = BufReader::new(f);
    // let mut cur =Cursor::new(rdr);
    let info = mdfinfo::mdfinfo(&mut rdr);

    Ok(())

}
