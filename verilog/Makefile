
out.yaml: output.json
	python parse.py

output.json: input.v
	yosys -p "synth_ice40 -nobram -nodffe -nocarry -json output.json" input.v

.PHONY: clean
clean:
	rm -f output.json out.yaml

