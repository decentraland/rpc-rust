# Run integration example
run-integration transport="":
  cd examples/integration && cargo run -- {{transport}}

# Run multi language integration example
run-multilang:
  cd examples/integration-multi-lang && cargo run -q > /dev/null &
  sleep 8;
  cd examples/integration-multi-lang/rpc-client-ts && npm i && npm start
  -@ps aux | grep "cargo run -q" | awk '{print $2}' | tail -1 | xargs kill -9 # kills server

# Generate docs for dcl-rpc and open it on browser
docs:
  cargo doc --no-deps --package "dcl-rpc" --open --document-private-items