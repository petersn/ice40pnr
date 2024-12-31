use std::{collections::HashMap, fmt::Write};

use crate::{chipdb::{ChipDb, ConfiguredArc, TilePos}, pnr::{IoPinSpot, PnrProblem, PnrSolution}};

pub struct BitMatrix {
  pub rows: usize,
  pub cols: usize,
  pub data: Vec<bool>,
}

impl BitMatrix {
  fn assert_valid(&self) {
    assert_eq!(self.data.len(), self.rows * self.cols);
  }

  pub fn serialize(&self, w: &mut impl Write) -> std::fmt::Result {
    self.assert_valid();
    for row in 0..self.rows {
      for col in 0..self.cols {
        w.write_char(if self.data[row * self.cols + col] { '1' } else { '0' })?;
      }
      w.write_char('\n')?;
    }
    Ok(())
  }
}

pub struct BitStreamEntry {
  pub name: String,
  pub args: Vec<String>,
  pub matrix: BitMatrix,
}

pub struct BitStream {
  pub entries: Vec<BitStreamEntry>,
  pub tile_to_entry_index: HashMap<TilePos, usize>,
}

impl BitStream {
  fn from_entries(entries: Vec<BitStreamEntry>) -> Self {
    let tile_like = [
      "io_tile",
      "logic_tile",
      "ramb_tile",
      "ramt_tile",
      "dsp0_tile",
      "dsp1_tile",
      "dsp2_tile",
      "dsp3_tile",
      "ipcon_tile",
    ];
    let mut tile_to_entry_index = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
      if tile_like.contains(&&entry.name[..]) {
        assert_eq!(entry.args.len(), 2);
        let xy = TilePos(
          entry.args[0].parse().unwrap(),
          entry.args[1].parse().unwrap(),
        );
        assert_eq!(tile_to_entry_index.insert(xy, i), None);
      }
    }
    BitStream { entries, tile_to_entry_index }
  }

  pub fn set_bit_row_col(&mut self, xy: TilePos, row: usize, col: usize) {
    println!(" -- Setting bit for tile {:?} at [{}, {}]", xy, row, col);
    let entry_index = match self.tile_to_entry_index.get(&xy) {
      Some(&i) => i,
      None => panic!("No entry for tile {:?}", xy),
    };
    let entry = &mut self.entries[entry_index];
    assert!(row < entry.matrix.rows);
    assert!(col < entry.matrix.cols);
    let bit_index = row * entry.matrix.cols + col;
    assert!(!entry.matrix.data[bit_index]);
    entry.matrix.data[bit_index] = true;
  }

  pub fn set_bit(&mut self, xy: TilePos, bit_desc: &str) {
    // The desc is always like "B<row>[<col>]".
    assert!(bit_desc.starts_with('B'));
    let mut parts = bit_desc[1..].split('[');
    let row: usize = parts.next().unwrap().parse().unwrap();
    let col: usize = parts.next().unwrap().trim_end_matches(']').parse().unwrap();
    self.set_bit_row_col(xy, row, col);
  }
}

pub fn parse(content: &str) -> Result<BitStream, String> {
  let mut entries = Vec::new();
  let mut lines = content.lines().peekable();
  while let Some(mut line) = lines.next() {
    line = line.trim();
    if line.is_empty() {
      continue;
    }
    if !line.starts_with('.') {
      return Err(format!("Expected line to start with '.', got '{}'", line));
    }
    line = &line[1..];
    let mut parts = line.split_whitespace();
    let name = parts.next().unwrap();
    let args = parts.map(|s| s.to_string()).collect();
    let mut rows = 0;
    let mut cols = None;
    let mut data = Vec::new();
    while let Some(line) = lines.peek() {
      if line.is_empty() || line.starts_with('.') {
        break;
      }
      match cols {
        None => cols = Some(line.len()),
        Some(expected_cols) if line.len() != expected_cols => {
          return Err(format!("Expected {} columns, got {}", expected_cols, line.len()));
        }
        _ => {}
      }
      let line = lines.next().unwrap();
      for c in line.chars() {
        if c != '0' && c != '1' {
          return Err(format!("Invalid character in data block: '{}'", c));
        }
        data.push(c == '1');
      }
      rows += 1;
    }
    assert_eq!(data.len(), rows * cols.unwrap_or(0));
    entries.push(BitStreamEntry {
      name: name.to_string(),
      args,
      matrix: BitMatrix { rows, cols: cols.unwrap_or(0), data },
    });
  }
  Ok(BitStream::from_entries(entries))
}

pub fn serialize(bitstream: &BitStream, w: &mut impl Write) -> std::fmt::Result {
  let only_one_newline = [
    "comment",
    "device",
    "sym",
  ];
  for entry in &bitstream.entries {
    write!(w, ".{}", entry.name.as_str())?;
    for arg in &entry.args {
      write!(w, " {}", arg)?;
    }
    w.write_char('\n')?;
    entry.matrix.serialize(w)?;
    if !only_one_newline.contains(&&entry.name[..]) {
      w.write_char('\n')?;
    }
  }
  Ok(())
}

pub fn add_arcs_and_luts(
  bs: &mut BitStream,
  chipdb: &ChipDb,
  problem: &PnrProblem,
  solution: &PnrSolution,
) {
  // Configure LUTs.
  assert_eq!(problem.lut4s.len(), solution.lut_placements.len());
  let mut clock_domains = HashMap::new();
  let mut extra_arcs = Vec::new();
  for (lut, &(tile, lut_number)) in problem.lut4s.iter().zip(&solution.lut_placements) {
    println!("Configuring LUT {:?} at {:?} with table {:016b}", lut, tile, lut.table);
    // Set all bits for the actual lookup table.
    let table_bit_row_offset_and_column = [
      (0, 40), (1, 40), (1, 41), (0, 41), (0, 42), (1, 42), (1, 43), (0, 43),
      (0, 39), (1, 39), (1, 38), (0, 38), (0, 37), (1, 37), (1, 36), (0, 36),
    ];
    // let carry_enable_row_offset_and_column = (0, 44);
    let dff_enable_row_offset_and_column = (0, 45);
    // let set_no_reset_row_offset_and_column = (1, 44);
    // let async_set_row_offset_and_column = (1, 45);
    for i in 0..16 {
      if (lut.table >> i) & 1 != 0 {
        let (row_offset, column) = table_bit_row_offset_and_column[i];
        let row = 2 * lut_number as usize + row_offset;
        println!(" Setting bit for LUT {:016b}: {:?}[{}][{}]", lut.table, tile, row, column);
        bs.set_bit_row_col(tile, row, column);
      }
    }
    if let Some(clock_domain) = lut.clock_domain {
      let (row_offset, column) = dff_enable_row_offset_and_column;
      let row = 2 * lut_number as usize + row_offset;
      println!(" Setting bit for LUT DFF enable: {:?}[{}][{}]", tile, row, column);
      bs.set_bit_row_col(tile, row, column);
      match clock_domains.insert(tile, clock_domain) {
        None => {
          // For now I only support the global clock networks.
          assert!(clock_domain < 8);
          let from = chipdb.get_net_by_name(TilePos(1, 1), &format!("glb_netwk_{}", clock_domain)).unwrap();
          let to = chipdb.get_net_by_name(tile, "lutff_global/clk").unwrap();
          extra_arcs.push(chipdb.get_configured_arc_between(from, to).unwrap());
        }
        Some(old) => assert_eq!(old, clock_domain),
      }
    }
  }

  // Configure arcs.
  for arc_source in [&solution.configured_arcs, &extra_arcs] {
    for arc in arc_source {
      add_configured_arc(bs, chipdb, *arc);
    }
  }
}

pub fn add_configured_arc(
  bs: &mut BitStream,
  chipdb: &ChipDb,
  arc: ConfiguredArc,
) {
  println!("Adding arc {:?} -> {:?}", arc.arc, arc.config_index);
  let arc_entry = &chipdb.arcs[arc.arc.0];
  let connection = &arc_entry.connections[arc.config_index];
  assert_eq!(arc_entry.config_bit_names.len(), connection.config_bits.len());
  for (name, bit) in arc_entry.config_bit_names.iter().zip(&connection.config_bits) {
    if *bit {
      println!("  Setting bit for arc {:?}: {}", arc_entry.xy, name);
      bs.set_bit(arc_entry.xy, name);
    }
  }
}

pub fn set_io_pin(
  bs: &mut BitStream,
  io_pin_spot: IoPinSpot,
  is_output: bool,
) -> Result<(), String> {
  println!("Setting IO pin {:?} as {}.", io_pin_spot, if is_output { "output" } else { "input" });
  let IoPinSpot { tile, which } = io_pin_spot;
  assert!(which == 0 || which == 1);
  // Set the bizarre "IoCtrl cf_bit_39" or "IoCtrl cf_bit_35" thing.
  bs.set_bit(tile, if which == 0 { "B6[15]" } else { "B12[15]" });
  // Set IOB_{which} PINTYPE_0.
  bs.set_bit(tile, if which == 0 { "B3[17]" } else { "B13[17]" });
  // Set REN_{which}.
  bs.set_bit(tile, if which == 0 { "B1[3]" } else { "B6[2]" });
  if is_output {
    // Set IOB_{which} PINTYPE_3.
    bs.set_bit(tile, if which == 0 { "B0[16]" } else { "B10[16]" });
    // Set IOB_{which} PINTYPE_4.
    bs.set_bit(tile, if which == 0 { "B4[16]" } else { "B14[16]" });
  } else {
    // Set IE_{which}.
    bs.set_bit(tile, if which == 0 { "B6[3]" } else { "B9[3]" });
  }
  Ok(())
}
