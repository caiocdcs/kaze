use std::convert::Infallible;

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt as _, future::ready};
use tokio_stream::wrappers::BroadcastStream;

use super::AppState;

/// Live events as Server-Sent Events. Each message is the JSON of
/// `events::Event`, whose `type` field says what happened.
pub async fn stream(
    State(app): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = BroadcastStream::new(app.ctx.events.subscribe())
        // A client that falls behind skips events rather than erroring.
        .filter_map(|received| ready(received.ok()))
        .filter_map(|event| ready(Event::default().json_data(event).ok()))
        .map(Ok)
        // End the stream on shutdown, or graceful shutdown would wait forever.
        .take_until(app.shutdown_token().cancelled_owned());
    Sse::new(events).keep_alive(KeepAlive::default())
}
