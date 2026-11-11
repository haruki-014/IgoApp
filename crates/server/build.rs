// マイグレーションは sqlx::migrate! でバイナリに埋め込む。
// SQL ファイルを追加・変更したときに再ビルドされるようにする。
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
