//! Manual probe: exercise the GitHub client exactly as the worker does.
//! Run: cargo run --example ping

fn main() {
    let client = rootle::github::Client::new();
    println!("anonymous: {}", client.is_anonymous());

    let t0 = std::time::Instant::now();
    match client.search("ratatui") {
        Ok(items) => println!("search ok in {:?}: {} items", t0.elapsed(), items.len()),
        Err(e) => println!("search ERR in {:?}: {e}", t0.elapsed()),
    }

    let t1 = std::time::Instant::now();
    match client.org_repos("ratatui") {
        Ok(repos) => println!("org ok in {:?}: {} repos", t1.elapsed(), repos.len()),
        Err(e) => println!("org ERR in {:?}: {e}", t1.elapsed()),
    }
}
