ice40pnr
========

A super duper simplified and minimal (<850 lines of Rust) example of generating valid iCE40 bitstream files from a description of the desired LUT4s and connectivity.
This repo would be impossible without the incredible work of [Project IceStorm](https://github.com/YosysHQ/icestorm).
I directly use their chip database files that enumerate the entire routing fabric of iCE40 parts.


Usage
-----

To build the ice40pnr file:
```
cargo build --release
```
Then, to synthesize an example input:
```
./target/release/ice40pnr examples/counter.yaml -o output.asc
```
This results in a `.asc` bitstream file, which you can flash onto an iCE40 FPGA using project IceStorm, as follows:
```
icepack output.asc output.bin
iceprog -d i:0x0403:0x6014 output.bin   # Or whatever the right -d is for your board.
```

Right now this project only targets the ice40up5k part (but should be very easily retargetable), and in particular the examples assume the [UPduino v3.1](https://tinyvision.ai/products/fpga-development-board-upduino-v3-1), but could be trivially retargeted to another board by just changing which pins are in use.


Input file format
-----------------

An input file simply specifies all of the IO pins in use, the LUT4s in use, and the connectivity.
Here's an example file showing all of the features:
```yaml
# Note: These IO pins are assuming the UPduino v3.1. You might need to change them.
used_ios:
  - # Declare the 12 MHz clock pin as an input.
    spot: # This is the spot for pin 20 on the sg48 package.
      tile: [19, 0]
      which: 1
    is_output: false
  - # Declare the LED output pin as an output.
    spot: # This is the spot for pin 41 on the sg48 package.
      tile: [6, 31]
      which: 0
    is_output: true

lut4s:
  -
    # Define the 16-bit table for this 4-bit -> 1-bit LUT.
    table: 0x1234
    # For simple combinational LUTs, set clock_domain to null:
    clock_domain: null
  -
    # For example, this is a 2-input XOR gate.
    table: 0b0110
    # A LUT4 may optionally have a flipflop afterwards.
    # If so, you specify a clock domain, 0 through 7, and the
    # flipflop will be hooked up to that global clock network.
    clock_domain: 7

# Each wire has a from and a to.
# The from must be either Pin, or Lut, and to must be Pin, Lut, or GlobalNetIngress.
# I give examples of all of these cases below:
wires:
  # Simplest case: Hook up the output from the first LUT into the second LUT's second input.
  -
    from:
      type: Lut
      lut_index: 0
    to:
      type: Lut
      lut_index: 0
      input_index: 1

  # You can also route to an output pin (in this case pin 41), like this:
  -
    from:
      type: Lut
      lut_index: 1
    to:
      type: Pin
      tile: [6, 31]
      which: 0

  # Finally, you can route from somewhere to the global clock networks.
  # Each global clock network has a particular tile it ingresses at,
  # and for network 7 it happens to be (19, 0).
  -
    from:
      type: Pin
      tile: [19, 0]
      which: 1
    to:
      type: GlobalNetIngress
      tile: [19, 0]
```

Look in `examples/` for real examples doing useful stuff.

License
-------

This entire project (except for the two asset files in `assets/`, which are from Project IceStorm) is CC0/public domain.
