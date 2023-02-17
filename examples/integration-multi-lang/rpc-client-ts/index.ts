import { createRpcClient } from "@dcl/rpc"
import { WebSocketTransport } from "@dcl/rpc/dist/transports/WebSocket"
import { loadService } from "@dcl/rpc/dist/codegen"
import { WebSocket } from 'ws';
import { Book, BookServiceDefinition, GetBookRequest } from "./api";
import expect from "expect";

console.log("> Creating WebSocket client")
let ws = new WebSocket('ws://127.0.0.1:8080');
const clientSocket = WebSocketTransport(ws)

console.log("> Creating RPC client")
const clientPromise = createRpcClient(clientSocket)

async function* bookRequestGenerator() {
    for (let i = 0; i < 5; i++) {
        const request: GetBookRequest = { isbn: (1000+i) }
        yield request  
    }
  }
  

async function handleClientCreation() {
  const client = await clientPromise
  console.log("  Client created!")
  console.log("> Creating client port")
  const clientPort = await client.createPort("my-port")
  console.log("> Requesting BookService client")
  const clientBookService = loadService(clientPort, BookServiceDefinition)

  console.log("> Unary > Invoking BookService.getBook(isbn:1001)")
  const response = await clientBookService.getBook({ isbn: 1001 })
  console.log("  Response: ", response)
  expect(response).toEqual({
    author: "mr jobs",
    title: "Rust: how do futures work under the hood?",
    isbn: 1001
  });


  console.log("> Server stream > Invoking BookService.queryBooks(authorPrefix:'mr')")
  const list: Book[] = []
  for await (const book of clientBookService.queryBooks({ authorPrefix: "mr" })) {
    list.push(book)
    console.log(book)
  }
  expect(list.length).toBe(3)

  console.log("> Client stream > Invoking BookService.getBookStream(bookRequestGenerator())")
  const streamResponse = await clientBookService.getBookStream(bookRequestGenerator())
  console.log("  Response: ", streamResponse)
  expect(streamResponse).toEqual({
    author: "mr steve",
    title: "Rust: crash course",
    isbn: 1000,
  })

  console.log("> Bidirectional stream > Invoking BookService.queryBooksStream(bookRequestGenerator())")
  const list_bidir: Book[] = [];
  for await (const book of clientBookService.queryBooksStream(bookRequestGenerator())) {
    console.log(book)
    list_bidir.push(book);
  }
  expect(list_bidir.length).toBe(5)
  process.exit(0)
}

handleClientCreation().catch((err) => {
  process.exitCode = 1
  console.error(err)
})