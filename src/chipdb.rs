use std::collections::HashMap;

use serde::Deserialize;

use crate::pnr::IoPinSpot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChipNetIndex(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArcIndex(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub struct TilePos(pub u8, pub u8);

impl TilePos {
  pub fn from_usize(x: usize, y: usize) -> Self {
    assert!(x < 256);
    assert!(y < 256);
    TilePos(x as u8, y as u8)
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConfiguredArc {
  pub arc: ArcIndex,
  pub config_index: usize,
}

#[derive(Debug, Clone)]
pub struct ChipNetEntry {
  pub net_index: ChipNetIndex,
  pub locations: Vec<(TilePos, String)>,
}

#[derive(Debug, Clone)]
pub struct ArcEntry {
  pub arc_index: ArcIndex,
  pub is_buffer: bool,
  pub xy: TilePos,
  pub config_bit_names: Vec<String>,
  pub dest: ChipNetIndex,
  pub connections: Vec<Connection>,
}

#[derive(Debug, Clone)]
pub struct Connection {
  pub config_bits: Vec<bool>,
  pub source: ChipNetIndex,
}

#[derive(Debug, Clone)]
pub struct PackagePins {
  pub pin_name_to_pos_and_index: HashMap<String, (TilePos, u8)>,
}

#[derive(Debug)]
pub struct ChipDb {
  pub nets: Vec<ChipNetEntry>,
  pub arcs: Vec<ArcEntry>,
  pub froms: HashMap<ChipNetIndex, Vec<(ChipNetIndex, ConfiguredArc)>>,
  pub net_by_name: HashMap<(TilePos, String), ChipNetIndex>,
  pub logic_tiles: Vec<TilePos>,
  pub pins_by_package: HashMap<String, PackagePins>,
}

impl ChipDb {
  pub fn parse(content: &str) -> Result<Self, String> {
    let mut nets = Vec::new();
    let mut arcs = Vec::new();
    let mut logic_tiles = Vec::new();
    let mut pins_by_package = HashMap::new();
    let mut lines = content.lines();

    enum State {
      Pins(String, PackagePins),
      Net(ChipNetEntry),
      Arc(ArcEntry),
    }
    let mut state = None;

    macro_rules! finish_state {
      () => {
        match state.take() {
          Some(State::Pins(package_name, package_pins)) => {
            pins_by_package.insert(package_name, package_pins);
          }
          Some(State::Net(net)) => {
            assert_eq!(net.net_index.0, nets.len());
            nets.push(net);
          }
          Some(State::Arc(arc)) => {
            arcs.push(arc);
          }
          None => {}
        }
      };
    }

    while let Some(line) = lines.next() {
      let line = line.split('#').next().unwrap().trim();
      if line.is_empty() {
        continue;
      }

      let mut line_chunks = line.split_whitespace();
      let first = line_chunks.next().unwrap();
      match first {
        ".pins" => {
          finish_state!();
          let package = line_chunks.next().unwrap().to_string();
          state = Some(State::Pins(package, PackagePins {
            pin_name_to_pos_and_index: HashMap::new(),
          }));
        }
        ".logic_tile" => {
          finish_state!();
          let x = line_chunks.next().unwrap().parse().unwrap();
          let y = line_chunks.next().unwrap().parse().unwrap();
          assert!(line_chunks.next().is_none());
          logic_tiles.push(TilePos(x, y));
        }
        ".net" => {
          finish_state!();
          let net_index = ChipNetIndex(line_chunks.next().unwrap().parse().unwrap());
          assert!(line_chunks.next().is_none());
          state = Some(State::Net(ChipNetEntry {
            net_index,
            locations: Vec::new(),
          }));
        }
        ".buffer" | ".routing" => {
          finish_state!();
          let is_buffer = first == ".buffer";
          let x = line_chunks.next().unwrap().parse().unwrap();
          let y = line_chunks.next().unwrap().parse().unwrap();
          let dest = ChipNetIndex(line_chunks.next().unwrap().parse().unwrap());
          let config_bit_names = line_chunks.map(|s| s.to_string()).collect();
          state = Some(State::Arc(ArcEntry {
            arc_index: ArcIndex(arcs.len()),
            is_buffer,
            xy: TilePos(x, y),
            dest,
            config_bit_names,
            connections: Vec::new(),
          }));
        }
        x if x.starts_with('.') => {
          finish_state!();
        }
        _ => match &mut state {
          Some(State::Pins(_, pins)) => {
            let pin_name = first.to_string();
            let x = line_chunks.next().unwrap().parse().unwrap();
            let y = line_chunks.next().unwrap().parse().unwrap();
            let index = line_chunks.next().unwrap().parse().unwrap();
            assert!(line_chunks.next().is_none());
            let old = pins.pin_name_to_pos_and_index.insert(pin_name, (TilePos(x, y), index));
            if old.is_some() {
              return Err(format!("Duplicate pin name: {}", first));
            }
          }
          Some(State::Net(net)) => {
            let x = first.parse().unwrap();
            let y = line_chunks.next().unwrap().parse().unwrap();
            let name = line_chunks.next().unwrap();
            assert!(line_chunks.next().is_none());
            net.locations.push((TilePos(x, y), name.to_string()));
          }
          Some(State::Arc(arc)) => {
            let config_bits = first.chars().map(|s| match s {
              '0' => false,
              '1' => true,
              _ => panic!("Invalid config bit: {}", s),
            }).collect();
            let source = ChipNetIndex(line_chunks.next().unwrap().parse().unwrap());
            assert!(line_chunks.next().is_none());
            arc.connections.push(Connection { config_bits, source });
          }
          None => {}
        },
      }
    }
    finish_state!();

    let mut froms: HashMap<ChipNetIndex, Vec<(ChipNetIndex, ConfiguredArc)>> = HashMap::new();
    for arc in &arcs {
      for (config_index, conn) in arc.connections.iter().enumerate() {
        let from = conn.source;
        let configured_arc = ConfiguredArc {
          arc: arc.arc_index,
          config_index,
        };
        froms.entry(arc.dest).or_default().push((from, configured_arc));
      }
    }

    let mut net_by_name = HashMap::new();
    for (i, net) in nets.iter().enumerate() {
      for (xy, name) in &net.locations {
        let old = net_by_name.insert((*xy, name.clone()), ChipNetIndex(i));
        if old.is_some() {
          return Err(format!("Duplicate net name: {}", name.as_str()));
        }
      }
    }

    Ok(ChipDb { nets, arcs, froms, net_by_name, logic_tiles, pins_by_package })
  }

  pub fn get_io_pin_spot(&self, package: &str, pin_name: &str) -> IoPinSpot {
    let pins = &self.pins_by_package[package].pin_name_to_pos_and_index;
    let &(tile, which) = pins.get(pin_name).unwrap();
    IoPinSpot { tile, which }
  }

  pub fn ff_out(&self, tile: TilePos, lut_number: u8) -> Result<ChipNetIndex, String> {
    self.get_net_by_name(tile, &format!("lutff_{}/out", lut_number))
  }

  pub fn ff_in(&self, tile: TilePos, lut_number: u8, input_index: u8) -> Result<ChipNetIndex, String> {
    self.get_net_by_name(tile, &format!("lutff_{}/in_{}", lut_number, input_index))
  }

  // These two look backwards, but are correct -- an IO tile for an input pin is the one with an output wire.
  pub fn io_tile_out(&self, io_pin_spot: IoPinSpot) -> Result<ChipNetIndex, String> {
    let IoPinSpot { tile, which } = io_pin_spot;
    self.get_net_by_name(tile, &format!("io_{}/D_IN_0", which))
  }

  pub fn io_tile_in(&self, io_pin_spot: IoPinSpot) -> Result<ChipNetIndex, String> {
    let IoPinSpot { tile, which } = io_pin_spot;
    self.get_net_by_name(tile, &format!("io_{}/D_OUT_0", which))
  }

  pub fn get_net_by_name(&self, tile: TilePos, name: &str) -> Result<ChipNetIndex, String> {
    let k = (tile, name.to_string());
    self.net_by_name.get(&k).copied().ok_or_else(|| format!("Net not found: {:?}", k))
  }

  pub fn get_configured_arc_between(&self, from: ChipNetIndex, to: ChipNetIndex) -> Option<ConfiguredArc> {
    for (potential_from, configured_arc) in &self.froms[&to] {
      if *potential_from == from {
        return Some(*configured_arc);
      }
    }
    None
  }

  pub fn get_global_net_ingress_point(&self, global_net_index: u8) -> Result<ChipNetIndex, String> {
    match global_net_index {
      7 => self.get_net_by_name(TilePos(19, 0), "fabout"),
      _ => Err(format!("Global net index out of range: {}", global_net_index)),
    }
  }
}
