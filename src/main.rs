use std::process::Stdio;
use std::sync::Arc;

use futures::prelude::*;
use log::debug;
use tokio::prelude::*;
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, Mutex};
use warp::filters::ws::{Message, WebSocket};
use warp::Filter;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let mut child = Command::new("python")
                            .arg("-i")
                            .stdin(Stdio::piped())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn()
                            .expect("A command failed to start");

    let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
    let stdout = Arc::new(Mutex::new(child.stdout.take().unwrap()));
    let stderr = Arc::new(Mutex::new(child.stderr.take().unwrap()));
    let child = Arc::new(Mutex::new(child));

    let ws = warp::path("ws")
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .and(warp::any().map(move || child.clone()))
        .and(warp::any().map(move || stdin.clone()))
        .and(warp::any().map(move || stdout.clone()))
        .and(warp::any().map(move || stderr.clone()))
        .map(|ws: warp::ws::Ws, child, stdin, stdout, stderr| {
            // And then our closure will be called when it completes...
            ws.on_upgrade(move |websocket| user_connected(websocket, child, stdin, stdout, stderr))
        });
    let index = warp::path::end().map(|| warp::reply::html(INDEX_HTML));
    let routes = index.or(ws);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn user_connected(
    ws: WebSocket,
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<ChildStdout>>,
    stderr: Arc<Mutex<ChildStderr>>,
) {
    let (ws_tx, mut ws_rx) = ws.split();

    let (tx, rx) = mpsc::unbounded_channel();
    tokio::task::spawn(rx.forward(ws_tx).map(|result| {
        if let Err(e) = result {
            eprintln!("websocket send error: {}", e);
        }
    }));

    let tx2 = tx.clone();
    tokio::task::spawn(async move {
        let mut buf: [u8; 10] = [0; 10];
        loop {
            debug!("loop");
            let mut stdout = stdout.lock().await;
            debug!("locked");
            let n = stdout.read(&mut buf[..]).await.unwrap();
            debug!("Read {} bytes", n);
            let msg = Message::text(std::str::from_utf8(&buf[..]).unwrap());
            if let Err(_disconnected) = tx.send(Ok(msg)) {
                // The tx is disconnected, our `user_disconnected` code
                // should be happening in another task, nothing more to
                // do here.
            }
        }
    });

    tokio::task::spawn(async move {
        let mut buf: [u8; 10] = [0; 10];
        loop {
            debug!("loop");
            let mut stderr = stderr.lock().await;
            debug!("locked");
            let n = stderr.read(&mut buf[..]).await.unwrap();
            debug!("Read {} bytes", n);
            let msg = Message::text(std::str::from_utf8(&buf[..]).unwrap());
            if let Err(_disconnected) = tx2.send(Ok(msg)) {
                // The tx2 is disconnected, our `user_disconnected` code
                // should be happening in another task, nothing more to
                // do here.
            }
        }
    });

    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("websocket error: {}", e);
                break;
            }
        };
        debug!("received {:?}", msg);
        let mut stdin = stdin.lock().await;
        debug!("locked");
        stdin.write_all(msg.as_bytes()).await.unwrap();
        debug!("1st write");
        stdin.write_all(b"\n").await.unwrap();
        debug!("2nd write");
        stdin.flush().await.unwrap();
        debug!("flush");
    }
}

static INDEX_HTML: &str = r#"
<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8">
    <title>PoC</title>
  </head>
  <body>
    <h1>PoC</h1>
    <div id="ws-status">Disconnected</div>
    <input id="stdin" type="text" placeholder="stdin" autofocus>
    <script type="text/javascript">
    var uri = 'ws://' + location.host + '/ws';
    var ws = new WebSocket(uri);
    function message(data) {
      var line = document.createElement('p');
      line.innerText = data;
      document.body.appendChild(line);
    }
    ws.onopen = function() {
      document.querySelector('#ws-status').innerText = "Connected";
    }
    ws.onmessage = function(msg) {
      message(msg.data);
    };
    document.querySelector('#stdin').addEventListener('keydown', e => {
      if (e.key === 'Enter') {
        var msg = e.target.value;
        ws.send(msg);
        e.target.value = '';
      }
    });
    </script>
  </body>
</html>
"#;
