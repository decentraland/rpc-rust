# Run integration example
run-integration transport="":
  cd examples/integration && cargo run -- {{transport}}

# Run multi language integration example
run-multilang:
  cd examples/integration-multi-lang && cargo run -q > /dev/null &
  sleep 8;
  cd examples/integration-multi-lang/rpc-client-ts && npm i && npm start