use crate::{
    Book, BookServiceServer, ClientStreamRequest, GetBookRequest, MyExampleContext,
    QueryBooksRequest, ServerStreamResponse,
};
use std::{sync::Arc, time::Duration};

use dcl_rpc::{stream_protocol::Generator, transports::web_socket::WebSocketTransport};
use tokio::time::sleep;

pub struct BookService {}

#[async_trait::async_trait]
impl BookServiceServer<MyExampleContext, WebSocketTransport> for BookService {
    async fn get_book(
        &self,
        request: GetBookRequest,
        ctx: Arc<MyExampleContext>,
        _: Arc<WebSocketTransport>,
    ) -> Book {
        assert_eq!(ctx.hardcoded_database.len(), 5);

        // Simulate DB operation
        println!(
            "> BookService > async get_book {} > simulating DB operation",
            request.isbn
        );
        sleep(Duration::from_secs(2)).await;
        let book = ctx
            .hardcoded_database
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
        _: Arc<WebSocketTransport>,
    ) -> ServerStreamResponse<Book> {
        println!("> BookService > server stream > QueryBooks");
        let (generator, generator_yielder) = Generator::create();
        // Spawn for a quick response
        tokio::spawn(async move {
            for book in &ctx.hardcoded_database {
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
        _: Arc<WebSocketTransport>,
    ) -> Book {
        while request.next().await.is_some() {}

        ctx.hardcoded_database[0].clone()
    }

    async fn query_books_stream(
        &self,
        mut request: ClientStreamRequest<GetBookRequest>,
        ctx: Arc<MyExampleContext>,
        _: Arc<WebSocketTransport>,
    ) -> ServerStreamResponse<Book> {
        println!("> BookService > bidir stream > QueryBooksStream");
        let (generator, generator_yielder) = Generator::create();
        // Spawn for a quick response
        tokio::spawn(async move {
            while let Some(message) = request.next().await {
                let book = ctx
                    .hardcoded_database
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
