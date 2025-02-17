module top(
    input wire clock,
    output wire led
);
    // I'm concerned about this not closing timing:
    //reg [23:0] counter = 0;
    //always @(posedge clock) begin
    //    counter <= counter + 1;
    //end
    //assign led = counter[23];

    // ... so here's a version that should be shallower:
    reg [7:0] counter0 = 0;
    reg [7:0] counter1 = 0;
    reg [7:0] counter2 = 0;
    wire zeros0 = counter0 == 0;
    wire zeros1 = counter1 == 0;
    always @(posedge clock) begin
        counter0 <= counter0 + 1;
	counter1 <= counter1 + zeros0;
	counter2 <= counter2 + (zeros0 && zeros1);
    end
    assign led = counter2[7];
endmodule
