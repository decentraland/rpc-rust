use std::{sync::Arc, time::Duration};

use tokio::time::sleep;

use crate::{codegen::server::BookServiceInterface, MyExampleContext};

use super::api::Book;

pub struct BookService {}

#[async_trait::async_trait]
impl BookServiceInterface<MyExampleContext> for BookService {
    async fn get_book(
        &self,
        request: super::api::GetBookRequest,
        ctx: Arc<MyExampleContext>,
    ) -> super::api::Book {
        assert_eq!(ctx.hardcoded_database.len(), 2);

        // Simulate DB operation
        println!("> BookService > async get_book > simulating DB operation");
        sleep(Duration::from_secs(1)).await;
        let book = ctx
            .hardcoded_database
            .iter()
            .find(|book_record| book_record.isbn == request.isbn);
        println!("> BookService > async get_book > awaited DB operation");

        book.map(Book::clone).unwrap_or_default()
    }
}
