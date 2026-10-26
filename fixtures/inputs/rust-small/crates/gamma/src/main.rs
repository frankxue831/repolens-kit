use alpha::greet;
use beta::util::Helper;

fn main() {
    let msg = greet::hello();
    let h = Helper;
    println!("{} {}", msg, h.assist());
}
