use std::{collections::{HashMap, HashSet, VecDeque}, hash::Hash};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::Deserialize;
use crate::chipdb::{ChipDb, ChipNetIndex, ConfiguredArc, TilePos};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(transparent)]
pub struct LutIndex(pub usize);

#[derive(Debug, Deserialize)]
pub struct Lut4 {
  pub table: u16,
  pub clock_domain: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub struct IoPinSpot {
  pub tile: TilePos,
  pub which: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(tag = "type")]
pub enum OutputSpot {
  Pin(IoPinSpot),
  Lut {
    lut_index: LutIndex
  },
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "type")]
pub enum InputSpot {
  Pin(IoPinSpot),
  GlobalNetIngress {
    tile: TilePos,
  },
  Lut {
    lut_index: LutIndex,
    input_index: u8,
  },
}

#[derive(Debug, Deserialize)]
pub struct UsedIo {
  pub spot: IoPinSpot,
  pub is_output: bool,
}

#[derive(Debug, Deserialize)]
pub struct Wire {
  pub from: OutputSpot,
  pub to: InputSpot,
}

#[derive(Debug, Deserialize)]
pub struct PnrProblem {
  pub used_ios: Vec<UsedIo>,
  pub lut4s: Vec<Lut4>,
  pub wires: Vec<Wire>,
}

impl PnrProblem {
  pub fn new() -> Self {
    PnrProblem {
      used_ios: Vec::new(),
      lut4s: Vec::new(),
      wires: Vec::new(),
    }
  }
}

#[derive(Debug)]
pub struct PnrSolution {
  pub lut_placements: Vec<(TilePos, u8)>,
  pub configured_arcs: Vec<ConfiguredArc>,
}

fn dijkstra<K: Copy + Eq + Hash, E: Copy>(
  start: K,
  extra_starts: &[K],
  end: K,
  froms: &HashMap<K, Vec<(K, E)>>,
  consumed_chip_nets: &HashSet<K>,
) -> Option<Vec<E>> {
  let mut next: HashMap<K, (Option<E>, K, u32)> = HashMap::new();
  let mut visited: HashSet<K> = consumed_chip_nets.clone();
  let mut queue: VecDeque<K> = VecDeque::new();
  next.insert(end, (None, end, 0));
  queue.push_back(end);

  let start = 'main_loop: loop {
    let node = queue.pop_front()?;
    if let Some(preds) = froms.get(&node) {
      for (pred, edge) in preds {
        // Check if we've reached a start node.
        if *pred == start || extra_starts.contains(pred) {
          next.insert(*pred, (Some(*edge), node, 0 /* This cost is meaningless here */));
          break 'main_loop *pred;
        }
        if visited.contains(pred) {
          continue;
        }
        let cost = next[&node].2 + 1;
        match next.entry(*pred) {
          std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert((Some(*edge), node, cost));
            queue.push_back(*pred);
            visited.insert(*pred);
          }
          std::collections::hash_map::Entry::Occupied(mut entry) => {
            if cost < entry.get().2 {
              entry.insert((Some(*edge), node, cost));
              queue.push_back(*pred);
            }
          }
        }
      }
    }
  };

  // Reconstruct path
  let mut path = Vec::new();
  let mut node = start;
  while node != end {
    let (edge, pred, _) = next.get(&node)?;
    path.push(edge.unwrap());
    node = *pred;
  }

  Some(path)
}

pub fn place_and_route(
  chipdb: &ChipDb,
  problem: &PnrProblem,
) -> Result<PnrSolution, String> {
  let mut rng = StdRng::seed_from_u64(1234);
  let scale = (problem.lut4s.len() as f32 / 8.0).sqrt();
  let mut positions: Vec<(f32, f32)> = (0..problem.lut4s.len())
    .map(|_| (rng.gen_range(0.0..scale), rng.gen_range(0.0..scale)))
    .collect::<Vec<_>>();
  let logic_tiles_hashset: HashSet<TilePos> = chipdb.logic_tiles.iter().copied().collect();
  let mut examine_order = (0..problem.lut4s.len()).collect::<Vec<_>>();

  let timescale = 500.0;
  let base_luts_per_tile = 8;

  let chip_lut_count = chipdb.logic_tiles.len() * 8;
  if problem.lut4s.len() > chip_lut_count {
    return Err(format!("Too many LUTs: {} > {}", problem.lut4s.len(), chip_lut_count));
  }
  let exact_fit_capacity_factor = problem.lut4s.len() as f32 / chip_lut_count as f32;
  let capacity_factor = exact_fit_capacity_factor.max(0.5);
  let epochs = 10.0 + problem.lut4s.len() as f32 / 500.0;

  for iter in 0..(timescale * epochs) as i32 {
    let t = iter as f32 / timescale;
    let correction_factor = 1.0 - 0.8 * (-t).exp();
    let tug_factor = (0.2 * (-t).exp()).max(1e-4);
    let noise_factor = 1e-8;
    let tile_factor = 1.0 - (-0.05 * t).exp();
    let luts_per_tile = capacity_factor * base_luts_per_tile as f32 * (1.0 - 0.2 * (-0.1 * t).exp());

    examine_order.shuffle(&mut rng);

    // Slightly randomize the positions.
    for (x, y) in &mut positions {
      *x += noise_factor * rng.gen_range(-1.0..1.0);
      *y += noise_factor * rng.gen_range(-1.0..1.0);
    }

    // Place all LUTs into buckets.
    let mut buckets = HashMap::new();
    for (i, (x, y)) in positions.iter().enumerate() {
      let bucket = (x.floor() as i32, y.floor() as i32);
      buckets.entry(bucket).or_insert_with(Vec::new).push(i);
    }

    // Repel LUTs from each other.
    let lattice_spacing: f32 = 2.0f32.sqrt() / 3.0f32.powf(0.25);
    let desired_distance = lattice_spacing / luts_per_tile.sqrt();
    'lut_loop: for &i in &examine_order {
      let here = positions[i];
      let bucket = (here.0.floor() as i32, here.1.floor() as i32);
      let mut examined = 0;
      for dy in -1..=1 {
        for dx in -1..=1 {
          let new_bucket = (bucket.0 + dx, bucket.1 + dy);
          if let Some(other_bucket) = buckets.get(&new_bucket) {
            for &j in other_bucket {
              if i == j {
                continue;
              }
              examined += 1;
              if examined > 10 * 9 * base_luts_per_tile {
                continue 'lut_loop;
              }
              let there = positions[j];
              // Push the LUTs apart, down to a density of 8 LUTs per tile.
              let (mut dx, mut dy) = (there.0 - here.0, there.1 - here.1);
              let distance = (dx * dx + dy * dy).sqrt();
              if distance < desired_distance {
                let scale = correction_factor * (desired_distance / (1e-5 + distance) - 1.0);
                dx *= scale;
                dy *= scale;
                positions[i].0 -= dx / 2.0;
                positions[i].1 -= dy / 2.0;
                positions[j].0 += dx / 2.0;
                positions[j].1 += dy / 2.0;
              }
            }
          }
        }
      }
    }

    // Shift to stay in tiles.
    for (x, y) in &mut positions {
      let is_valid_tile = |x: i32, y: i32| (
        0 <= x && x < 256 && 0 <= y && y < 256 &&
        logic_tiles_hashset.contains(&TilePos(x as u8, y as u8))
      );
      let bucket = (x.floor() as i32, y.floor() as i32);
      if is_valid_tile(bucket.0, bucket.1) {
        continue;
      }
      let mut best: Option<((i32, i32), f32)> = None;
      macro_rules! try_bucket {
        ($bx:ident, $by:ident) => {{
          let bucket_center = ($bx as f32 + 0.5, $by as f32 + 0.5);
          let distance = (*x - bucket_center.0).abs() + (*y - bucket_center.1).abs();
          if is_valid_tile($bx, $by) && best.map_or(true, |(_, d)| distance < d) {
            best = Some((($bx, $by), distance));
          }
        }};
      }
      for dy in -1..=1 {
        for dx in -1..=1 {
          let (bx, by) = (bucket.0 + dx, bucket.1 + dy);
          try_bucket!(bx, by);
        }
      }
      // As a fallback, we just search for the closest valid tile globally.
      if best.is_none() {
        for tile in &chipdb.logic_tiles {
          let (bx, by) = (tile.0 as i32, tile.1 as i32);
          try_bucket!(bx, by);
        }
      }
      let ((bx, by), _) = best.unwrap();
      let (mut dx, mut dy) = (0.0, 0.0);
      if bx > bucket.0 {
        dx = bx as f32 - *x;
      } else if bx < bucket.0 {
        dx = (bx + 1) as f32 - *x;
      }
      if by > bucket.1 {
        dy = by as f32 - *y;
      } else if by < bucket.1 {
        dy = (by + 1) as f32 - *y;
      }
      *x += tile_factor * dx;
      *y += tile_factor * dy;
    }

    // Pull on edges.
    for Wire { from: output, to: input } in &problem.wires {
      let start = match output {
        OutputSpot::Pin(IoPinSpot { tile: pos, .. }) => (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5),
        OutputSpot::Lut { lut_index } => positions[lut_index.0],
      };
      let end = match input {
        InputSpot::Pin(IoPinSpot { tile: pos, .. })
        | InputSpot::GlobalNetIngress { tile: pos } => (pos.0 as f32 + 0.5, pos.1 as f32 + 0.5),
        InputSpot::Lut { lut_index, input_index: _ } => positions[lut_index.0],
      };
      let dx = end.0 - start.0;
      let dy = end.1 - start.1;
      let distance = (dx * dx + dy * dy).sqrt();
      if distance > 0.0 {
        let scale = tug_factor / (1.0 + distance);
        match output {
          OutputSpot::Pin(_) => {},
          OutputSpot::Lut { lut_index: LutIndex(index) } => {
            positions[*index].0 += dx * scale;
            positions[*index].1 += dy * scale;
          }
        };
        match input {
          InputSpot::Pin(_) => {},
          InputSpot::GlobalNetIngress { .. } => {},
          InputSpot::Lut { lut_index: LutIndex(index), input_index: _ } => {
            positions[*index].0 -= dx * scale;
            positions[*index].1 -= dy * scale;
          }
        };
      }
    }
  }

  // Assign LUTs to tiles, sorting by y.
  let mut luts_by_y: Vec<usize> = (0..problem.lut4s.len()).collect();
  luts_by_y.sort_by(|a, b| positions[*a].1.partial_cmp(&positions[*b].1).unwrap());
  let mut consumed_count = HashMap::new();
  let mut find_free = |x: f32, y: f32| {
    let mut best = None;
    // FIXME: Do a local search instead.
    for &tile in &chipdb.logic_tiles {
      // Check capacity.
      let consumed = consumed_count.get(&tile).copied().unwrap_or(0);
      if consumed >= base_luts_per_tile {
        continue;
      }
      let (tx, ty) = (tile.0 as f32 + 0.5, tile.1 as f32 + 0.5);
      let distance = (tx - x).abs() + (ty - y).abs();
      if best.map_or(true, |(_, d)| distance < d) {
        best = Some((tile, distance));
      }
    }
    let tile = best.unwrap().0;
    let lut_number = consumed_count.entry(tile).or_insert(0);
    let placement = (tile, *lut_number as u8);
    *lut_number += 1;
    placement
  };
  let mut lut_placements = vec![(TilePos(0, 0), 0); problem.lut4s.len()];
  for &i in &luts_by_y {
    let (x, y) = positions[i];
    let (tile, lut_number) = find_free(x, y);
    lut_placements[i] = (tile, lut_number);
  }

  let mut chip_nets_by_output: HashMap<OutputSpot, Vec<ChipNetIndex>> = HashMap::new();
  let mut consumed_chip_nets: HashSet<ChipNetIndex> = HashSet::new();
  let mut configured_arcs: Vec<ConfiguredArc> = Vec::new();
  for (i, &Wire { from, to }) in problem.wires.iter().enumerate() {
    if i % 100 == 0 {
      println!("Routing wire {}/{}", i, problem.wires.len());
    }
    let from_net = match from {
      OutputSpot::Pin(io_pin_spot) => chipdb.io_tile_out(io_pin_spot),
      OutputSpot::Lut { lut_index } => {
        let (tile, lut_number) = lut_placements[lut_index.0];
        chipdb.ff_out(tile, lut_number)
      }
    }?;
    let to_net = match to {
      InputSpot::Pin(io_pin_spot) => chipdb.io_tile_in(io_pin_spot),
      | InputSpot::GlobalNetIngress { tile: pos } => chipdb.get_net_by_name(pos, "fabout"),
      InputSpot::Lut { lut_index, input_index } => {
        let (tile, lut_number) = lut_placements[lut_index.0];
        chipdb.ff_in(tile, lut_number, input_index)
      }
    }?;
    let extra_starts = match chip_nets_by_output.get(&from) {
      Some(nets) => &nets[..],
      None => &[],
    };
    let Some(path) = dijkstra(from_net, extra_starts, to_net, &chipdb.froms, &consumed_chip_nets) else {
      let message = format!("No path found from {:?} to {:?}", from, to);
      return Err(message);
    };
    configured_arcs.extend(path.iter().copied());
    let chip_nets_for_this_output = chip_nets_by_output.entry(from).or_default();
    for edge in path {
      let arc = &chipdb.arcs[edge.arc.0 as usize];
      let arc_to = arc.dest;
      consumed_chip_nets.insert(arc_to);
      chip_nets_for_this_output.push(arc_to);
    }
  }
  println!("Routing complete");

  Ok(PnrSolution {
    lut_placements,
    configured_arcs,
  })
}
