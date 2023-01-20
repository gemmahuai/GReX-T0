mod capture;
mod if_packet;

fn main() {
    capture::UdpCap::new(9000);
}
