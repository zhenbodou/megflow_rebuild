use flow_derive::inputs;

#[inputs(inp: dyn dyn Send)]
struct TraitObjectPayload {}

fn main() {}
