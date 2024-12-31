pub mod chipdb;
pub mod pnr;
pub mod bitstream;

use std::path::PathBuf;
use clap::Parser;
use pnr::UsedIo;

/// Simple file processor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
  /// Input json file to process
  input_file: PathBuf,

  /// Output bitstream file
  #[arg(short, long)]
  output: PathBuf,
}

fn main() {
  let args = Args::parse();
  println!("Args: {:?}", args);

  // Load the input.
  let pnr_problem_str = std::fs::read_to_string(&args.input_file).unwrap();
  let pnr_problem: pnr::PnrProblem = serde_yaml::from_str(&pnr_problem_str).unwrap();
  println!("PnrProblem: {:#?}", pnr_problem);

  // Load the chipdb.
  let compressed = include_bytes!("../assets/chipdb-5k.txt.zst");
  let data_bytes = zstd::decode_all(&compressed[..]).unwrap();
  let data = std::str::from_utf8(&data_bytes).unwrap();
  let db = chipdb::ChipDb::parse(&data).unwrap();

  // Place and route the design.
  let solution = pnr::place_and_route(&db, &pnr_problem).unwrap();
  println!("PnrSolution: {:#?}", solution);

  // Assemble the final bitstream.
  let compressed = include_bytes!("../assets/empty.asc.zst");
  let data_bytes = zstd::decode_all(&compressed[..]).unwrap();
  let empty_asc = std::str::from_utf8(&data_bytes).unwrap();
  let mut bitstream = bitstream::parse(empty_asc).unwrap();
  bitstream::add_arcs_and_luts(&mut bitstream, &db, &pnr_problem, &solution);
  for UsedIo { spot, is_output } in pnr_problem.used_ios {
    bitstream::set_io_pin(&mut bitstream, spot, is_output).unwrap();
  }

  let mut s = String::new();
  bitstream::serialize(&bitstream, &mut s).unwrap();
  std::fs::write(&args.output, s).unwrap();
}
