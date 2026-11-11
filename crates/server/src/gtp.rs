//! GTP クライアント。エンジンとは TCP でつなぐ(エンジン側は接続ごとにプロセスを 1 つ起動する)。

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

#[derive(Debug, thiserror::Error)]
pub enum GtpError {
    #[error("接続エラー: {0}")]
    Io(#[from] std::io::Error),
    #[error("応答がタイムアウトしました")]
    Timeout,
    /// エンジンが `?` で始まるエラー応答を返した。
    #[error("エンジンがエラーを返しました: {0}")]
    Failure(String),
    #[error("GTP の応答を解釈できません: {0}")]
    Protocol(String),
}

impl GtpError {
    /// 接続そのものが切れている(つなぎ直せば直る見込みがある)エラーか。
    pub fn is_connection_error(&self) -> bool {
        matches!(self, GtpError::Io(_))
    }
}

pub struct GtpClient {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u32,
}

impl GtpClient {
    pub async fn connect(host: &str, port: u16, timeout: Duration) -> Result<GtpClient, GtpError> {
        let stream = tokio::time::timeout(timeout, TcpStream::connect((host, port)))
            .await
            .map_err(|_| GtpError::Timeout)??;
        let _ = stream.set_nodelay(true);
        let (reader, writer) = stream.into_split();
        Ok(GtpClient { reader: BufReader::new(reader), writer, next_id: 1 })
    }

    /// コマンドを 1 つ送り、応答の本文を返す。
    ///
    /// タイムアウトや解釈できない応答のあとは、接続の状態が分からなくなるので、
    /// 呼び出し側はこのクライアントを捨てること。
    pub async fn command(&mut self, command: &str, timeout: Duration) -> Result<String, GtpError> {
        tokio::time::timeout(timeout, self.exchange(command))
            .await
            .map_err(|_| GtpError::Timeout)?
    }

    async fn exchange(&mut self, command: &str) -> Result<String, GtpError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.writer.write_all(format!("{id} {command}\n").as_bytes()).await?;
        self.writer.flush().await?;

        // 応答は「=ID 本文」または「?ID エラー」で始まり、空行で終わる。
        let mut success = None;
        let mut body = String::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).await? == 0 {
                return Err(GtpError::Io(std::io::ErrorKind::UnexpectedEof.into()));
            }
            let line = line.trim_end_matches(['\r', '\n']);
            match success {
                None => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let (ok, rest) = if let Some(rest) = line.strip_prefix('=') {
                        (true, rest)
                    } else if let Some(rest) = line.strip_prefix('?') {
                        (false, rest)
                    } else {
                        return Err(GtpError::Protocol(line.to_string()));
                    };
                    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                    let (reply_id, text) = rest.split_at(digits);
                    if reply_id != id.to_string() {
                        return Err(GtpError::Protocol(format!("応答の ID が一致しません: {line}")));
                    }
                    success = Some(ok);
                    body.push_str(text.trim());
                }
                Some(_) => {
                    if line.trim().is_empty() {
                        break;
                    }
                    body.push('\n');
                    body.push_str(line);
                }
            }
        }
        let body = body.trim().to_string();
        if success == Some(true) { Ok(body) } else { Err(GtpError::Failure(body)) }
    }

    /// `quit` を送って接続を閉じる。失敗しても構わない。
    pub async fn quit(mut self) {
        let _ = self.command("quit", Duration::from_secs(2)).await;
    }
}
