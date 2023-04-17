use crate::{
    Book, BookServiceServer, ClientStreamRequest, GetBookRequest, MyExampleContext,
    QueryBooksRequest, ServerStreamResponse,
};
use dcl_rpc::{rpc_protocol::RemoteErrorResponse, stream_protocol::Generator};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

pub struct MyBookService {}

#[async_trait::async_trait]
impl BookServiceServer<MyExampleContext, BookServiceError> for MyBookService {
    async fn send_book(
        &self,
        book: Book,
        ctx: Arc<MyExampleContext>,
    ) -> Result<(), BookServiceError> {
        let mut books = ctx.hardcoded_database.write().await;

        // Simulate DB operation
        println!(
            "> BookService > async send_book {} > simulating DB operation",
            book.isbn
        );
        sleep(Duration::from_secs(2)).await;

        books.push(book);

        Ok(())
    }

    async fn get_sample_book(&self, _ctx: Arc<MyExampleContext>) -> Result<Book, BookServiceError> {
        Ok(Book {
            author: "mr jobs".to_string(),
            title: "Rust: how do futures work under the hood?".to_string(),
            isbn: 1001,
        })
    }

    async fn get_book(
        &self,
        request: GetBookRequest,
        ctx: Arc<MyExampleContext>,
    ) -> Result<Book, BookServiceError> {
        let books = ctx.hardcoded_database.read().await;

        // Simulate DB operation
        println!(
            "> BookService > async get_book {} > simulating DB operation",
            request.isbn
        );
        sleep(Duration::from_secs(2)).await;
        match books
            .iter()
            .find(|book_record| book_record.isbn == request.isbn)
        {
            Some(book) => {
                println!(
                    "> BookService > async get_book {} > after simulating DB operation",
                    request.isbn
                );

                Ok(book.clone())
            }
            None => Err(BookServiceError::BookNotFound),
        }
    }

    async fn query_books(
        &self,
        request: QueryBooksRequest,
        ctx: Arc<MyExampleContext>,
    ) -> Result<ServerStreamResponse<Book>, BookServiceError> {
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

        Ok(generator)
    }

    async fn get_book_stream(
        &self,
        mut request: ClientStreamRequest<GetBookRequest>,
        ctx: Arc<MyExampleContext>,
    ) -> Result<Book, BookServiceError> {
        let books = ctx.hardcoded_database.read().await;

        while request.next().await.is_some() {}

        Ok(books[0].clone())
    }

    async fn query_books_stream(
        &self,
        mut request: ClientStreamRequest<GetBookRequest>,
        ctx: Arc<MyExampleContext>,
    ) -> Result<ServerStreamResponse<Book>, BookServiceError> {
        println!("> BookService > bidir stream > QueryBooksStream");
        let (generator, generator_yielder) = Generator::create();
        // Spawn for a quick response
        tokio::spawn(async move {
            let books = ctx.hardcoded_database.read().await;
            while let Some(message) = request.next().await {
                let book = books.iter().find(|book| book.isbn == message.isbn);
                if let Some(book) = book {
                    sleep(Duration::from_millis(500)).await; // Simulating DB
                    generator_yielder.r#yield(book.clone()).await.unwrap()
                }
            }
        });
        Ok(generator)
    }
}

pub enum BookServiceError {
    BookNotFound,
}

impl RemoteErrorResponse for BookServiceError {
    fn error_code(&self) -> u32 {
        match self {
            Self::BookNotFound => 404,
        }
    }

    fn error_message(&self) -> String {
        match self {
            Self::BookNotFound => "Book wasn't found".to_string(),
        }
    }
}
