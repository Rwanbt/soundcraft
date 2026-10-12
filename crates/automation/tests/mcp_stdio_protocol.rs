//! Black-box stdio stream tests for the MCP server's JSON-RPC frame and batch handling.
//! No audio hardware, browser, plugins or external services are required.

use serde_json::{Value, json};
use soundcraft_automation::{Headless, mcp::Server};
use soundcraft_engine::Engine;
use std::io::Cursor;

fn serve(input: Vec<u8>) -> Vec<Value> {
    let mut server = Server::new(Headless::new(Engine::default()));
    let mut output = Vec::new();
    server.serve(Cursor::new(input), &mut output).unwrap();
    output.split(|&b| b == b'\n').filter(|frame| !frame.is_empty()).map(|frame| serde_json::from_slice(frame).unwrap()).collect()
}

#[test]
fn oversized_input_is_bounded_rejected_and_followed_by_a_valid_request() {
    let mut input = vec![b' '; 1024 * 1024];
    input.push(b'\n');
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n");
    let responses = serve(input);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], -32600);
    assert!(responses[0]["error"]["message"].as_str().unwrap().contains("1 MiB"));
    assert_eq!(responses[1], json!({"jsonrpc": "2.0", "id": 7, "result": {}}));
}

#[test]
fn exactly_one_mebibyte_with_newline_is_a_valid_jsonrpc_frame() {
    let command = b"{\"jsonrpc\":\"2.0\",\"id\":42,\"method\":\"ping\"}";
    let mut input = command.to_vec();
    input.resize(1024 * 1024 - 1, b' ');
    input.push(b'\n');
    let responses = serve(input);
    assert_eq!(responses, vec![json!({"jsonrpc": "2.0", "id": 42, "result": {}})]);
}

#[test]
fn malformed_utf8_does_not_break_stream_alignment() {
    let input =
        b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\",\"extra\":\"\xff\"}\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n".to_vec();
    let responses = serve(input);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1], json!({"jsonrpc": "2.0", "id": 3, "result": {}}));
}

#[test]
fn empty_batches_are_invalid_and_notification_only_batches_have_no_reply() {
    let input = concat!(
        "[]\n",
        "[{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}]\n",
        "[{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"},",
        "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}]\n"
    );
    let responses = serve(input.as_bytes().to_vec());
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["error"]["code"], -32600);
    let batch = responses[1].as_array().unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0], json!({"jsonrpc": "2.0", "id": 9, "result": {}}));
}

#[test]
fn final_frame_without_newline_is_processed_once_at_eof() {
    let responses = serve(b"{\"jsonrpc\":\"2.0\",\"id\":21,\"method\":\"ping\"}".to_vec());
    assert_eq!(responses, vec![json!({"jsonrpc": "2.0", "id": 21, "result": {}})]);
}
