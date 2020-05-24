use std::sync::{Arc};
use std::process::{Stdio};
use futures::prelude::*;
use tokio::sync::{Mutex};
use tokio::prelude::*;
use tokio::process::{Child, Command};
use warp::Filter;
use warp::filters::ws::{Message, WebSocket};

type MyChild = Arc<Mutex<Child>>;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let child = Command::new("tail").arg("-f").arg("test")
                        .stdin(Stdio::inherit())
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("A command failed to start");
    let child = Arc::new(Mutex::new(child));
    let child = warp::any().map(move || child.clone());
    let ws = warp::path("ws")
        // The `ws()` filter will prepare the Websocket handshake.
        .and(warp::ws())
        .and(child)
        .map(|ws: warp::ws::Ws, child| {
            // And then our closure will be called when it completes...
            ws.on_upgrade(move |websocket| user_connected(websocket, child))
        });
    let index = warp::path::end()
        .map(|| warp::reply::html(INDEX_HTML));
    let routes = index.or(ws);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;
}

async fn user_connected(ws: WebSocket, child: MyChild) {
    let (mut tx, _rx) = ws.split();

    let mut buf: [u8; 10] = [0; 10];
    loop {
        let n = child.lock().await.stdout.as_mut().unwrap().read(&mut buf[..]).await.unwrap();
        println!("Read {} bytes", n);
        let msg = Message::text(std::str::from_utf8(&buf[..]).unwrap());
        tx.send(msg).await.unwrap();
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
    <h1>test</h1>
    <script type="text/javascript">
    var uri = 'ws://' + location.host + '/ws';
    var ws = new WebSocket(uri);
    function message(data) {
      var line = document.createElement('p');
      line.innerText = data;
      document.body.appendChild(line);
    }
    ws.onopen = function() {
      document.body.innerHTML = "<p><em>Connected!</em></p>";
    }
    ws.onmessage = function(msg) {
      message(msg.data);
    };
    send.onclick = function() {
      var msg = text.value;
      ws.send(msg);
      text.value = '';
      message('<You>: ' + msg);
    };
    </script>
  </body>
</html>
"#;
