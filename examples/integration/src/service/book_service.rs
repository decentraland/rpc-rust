use crate::{
    Book, ClientStreamRequest, GetBookRequest, MyExampleContext, QueryBooksRequest,
    ServerStreamResponse, SharedBookService,
};
use std::{sync::Arc, time::Duration};

use dcl_rpc::stream_protocol::Generator;
use tokio::time::sleep;

pub struct MyBookService {}

#[async_trait::async_trait]
impl SharedBookService<MyExampleContext> for MyBookService {
    async fn send_book(&self, book: Book, ctx: Arc<MyExampleContext>) {
        let mut books = ctx.hardcoded_database.write().await;

        books.push(book);
    }

    async fn get_latest_book(&self, _ctx: Arc<MyExampleContext>) -> Book {
        // TODO
        Book {
            author: "mr jobs".to_string(),
            title: "Rust: how do futures work under the hood?".to_string(),
            isbn: 1001,
        }
    }

    async fn get_book(&self, request: GetBookRequest, ctx: Arc<MyExampleContext>) -> Book {
        let books = ctx.hardcoded_database.read().await;

        // Simulate DB operation
        println!(
            "> BookService > async get_book {} > simulating DB operation",
            request.isbn
        );
        sleep(Duration::from_secs(2)).await;
        let book = books
            .iter()
            .find(|book_record| book_record.isbn == request.isbn);
        println!(
            "> BookService > async get_book {} > after simulating DB operation",
            request.isbn
        );

        book.map(Book::clone).unwrap_or_default()
    }

    async fn query_books(
        &self,
        request: QueryBooksRequest,
        ctx: Arc<MyExampleContext>,
    ) -> ServerStreamResponse<Book> {

        println!("> BookService > server stream > QueryBooks");
        let (generator, generator_yielder) = Generator::create();
        // Spawn for a quick response
        tokio::spawn(async move {
            let books = ctx.hardcoded_database.read().await;
            for book in &*books {
                sleep(Duration::from_secs(1)).await;
                if book.author.contains(&request.author_prefix) {
                    generator_yielder.r#yield(book.clone()).await.unwrap();
                }
            }
        });

        generator
    }

    async fn get_book_stream(
        &self,
        mut request: ClientStreamRequest<GetBookRequest>,
        ctx: Arc<MyExampleContext>,
    ) -> Book {
        let books = ctx.hardcoded_database.read().await;

        while request.next().await.is_some() {}

        books[0].clone()
    }

    async fn query_books_stream(
        &self,
        mut request: ClientStreamRequest<GetBookRequest>,
        ctx: Arc<MyExampleContext>,
    ) -> ServerStreamResponse<Book> {

        println!("> BookService > bidir stream > QueryBooksStream");
        let (generator, generator_yielder) = Generator::create();
        // Spawn for a quick response
        tokio::spawn(async move {
            let books = ctx.hardcoded_database.read().await;
            while let Some(message) = request.next().await {
                let book = books
                    .iter()
                    .find(|book| book.isbn == message.isbn);
                if let Some(book) = book {
                    sleep(Duration::from_millis(500)).await; // Simulating DB
                    generator_yielder.r#yield(book.clone()).await.unwrap()
                }
            }
        });
        generator
    }
}
