import json
import yaml
import pprint

port_mapping = {
    "clock": {"type": "Pin", "tile": [19, 0], "which": 1},
    "led": {"type": "Pin", "tile": [6, 31], "which": 0},
}
port_is_output = {}

with open("output.json") as f:
    data = json.load(f)
luts = []
net_number_to_from_spot = {}
for port, port_data in data["modules"]["top"]["ports"].items():
    port_is_output[port] = port_data["direction"] == "output"
    bit, = port_data["bits"]
    if not port_is_output[port]:
        net_number_to_from_spot[bit] = port_mapping[port]
for k, v in data["modules"]["top"]["cells"].items():
    if v["type"] == "SB_DFF":
        clock, = v["connections"]["C"]
        data, = v["connections"]["D"]
        q, = v["connections"]["Q"]
        print(f"SB_DFF {q} (clock={clock}, data={data})")
        net_number_to_from_spot[q] = {
            "type": "Lut",
            "lut_index": len(luts),
        }
        luts.append(("dff", clock, data, q, len(luts)))
    elif v["type"] == "SB_LUT4":
        table = int(v["parameters"]["LUT_INIT"], 2)
        i0, = v["connections"]["I0"]
        i1, = v["connections"]["I1"]
        i2, = v["connections"]["I2"]
        i3, = v["connections"]["I3"]
        o, = v["connections"]["O"]
        print(f"SB_LUT4 {o} (I0={i0}, I1={i1}, I2={i2}, I3={i3})")
        net_number_to_from_spot[o] = {
            "type": "Lut",
            "lut_index": len(luts),
        }
        luts.append(("lut4", table, i0, i1, i2, i3, o, len(luts)))
    else:
        print("Unknown cell type", v["type"])

output = {
    "used_ios": [
        {
            "spot": port_mapping[port],
            "is_output": port_is_output[port],
        }
        for port in port_mapping
    ],
    "lut4s": [],
    "wires": [],
}
for lut in luts:
    if lut[0] == "dff":
        # _, clock, data, q = lut
        output["lut4s"].append({"table": 0b10, "clock_domain": 7})
    elif lut[0] == "lut4":
        _, table, i0, i1, i2, i3, o, lut_index = lut
        output["lut4s"].append({"table": table, "clock_domain": None})
for lut in luts:
    if lut[0] == "dff":
        _, clock, data, q, lut_index = lut
        output["wires"].append({
            "from": net_number_to_from_spot[data],
            "to": {
                "type": "Lut",
                "lut_index": lut_index,
                "input_index": 0,
            },
        })
    elif lut[0] == "lut4":
        _, table, i0, i1, i2, i3, o, lut_index = lut
        for i, x in enumerate([i0, i1, i2, i3]):
            if x == "0":
                continue
            output["wires"].append({
                "from": net_number_to_from_spot[x],
                "to": {
                    "type": "Lut",
                    "lut_index": lut_index,
                    "input_index": i,
                },
            })

with open("out.yaml", "w") as f:
    yaml.dump(output, f)

