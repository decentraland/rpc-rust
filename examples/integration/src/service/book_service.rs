use crate::{Book, GetBookRequest, MyExampleContext};
use std::{sync::Arc, time::Duration};

use tokio::time::sleep;

use crate::codegen::server::BookServiceInterface;

pub struct BookService {}

#[async_trait::async_trait]
impl BookServiceInterface<MyExampleContext> for BookService {
    async fn get_book(&self, request: GetBookRequest, ctx: Arc<MyExampleContext>) -> Book {
        assert_eq!(ctx.hardcoded_database.len(), 2);

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
            "> BookService > async get_book {} > simulating DB operation",
            request.isbn
        );

        book.map(Book::clone).unwrap_or_default()
    }
}
