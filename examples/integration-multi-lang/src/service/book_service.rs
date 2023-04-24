use crate::{
    Book, BookServiceServer, ClientStreamRequest, GetBookRequest, MyExampleContext,
    QueryBooksRequest, ServerStreamResponse,
};
use dcl_rpc::{rpc_protocol::RemoteErrorResponse, stream_protocol::Generator};
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

pub struct BookService {}

#[async_trait::async_trait]
impl BookServiceServer<MyExampleContext, BookServiceError> for BookService {
    async fn get_book(
        &self,
        request: GetBookRequest,
        ctx: Arc<MyExampleContext>,
    ) -> Result<Book, BookServiceError> {
        assert_eq!(ctx.hardcoded_database.len(), 5);

        // Simulate DB operation
        println!(
            "> BookService > async get_book {} > simulating DB operation",
            request.isbn
        );
        sleep(Duration::from_secs(2)).await;

        match ctx
            .hardcoded_database
            .iter()
            .find(|book_record| book_record.isbn == request.isbn)
        {
            Some(book) => {
                println!(
                    "> BookService > async get_book {} > after simulating DB operation",
                    book.isbn
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
            for book in &ctx.hardcoded_database {
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
        while request.next().await.is_some() {}

        Ok(ctx.hardcoded_database[0].clone())
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
            while let Some(message) = request.next().await {
                let book = ctx
                    .hardcoded_database
                    .iter()
                    .find(|book| book.isbn == message.isbn);
                if let Some(book) = book {
                    sleep(Duration::from_millis(200)).await; // Simulating DB
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
            BookServiceError::BookNotFound => 404,
        }
    }

    fn error_message(&self) -> String {
        match self {
            BookServiceError::BookNotFound => "Book wasn't found".to_string(),
        }
    }
}
