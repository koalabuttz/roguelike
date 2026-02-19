/// Quick diagnostic: can we read HID reports from the 8BitDo via libusb?
fn main() {
    use rusb::UsbContext;
    let ctx = rusb::Context::new().expect("usb context");
    let devices = ctx.devices().expect("device list");

    for device in devices.iter() {
        let desc: rusb::DeviceDescriptor = device.device_descriptor().unwrap();
        if desc.vendor_id() != 0x2dc8 || desc.product_id() != 0x6001 {
            continue;
        }
        println!("Found {:04x}:{:04x}", desc.vendor_id(), desc.product_id());

        let config = device.active_config_descriptor().unwrap();
        for iface in config.interfaces() {
            for iface_desc in iface.descriptors() {
                println!(
                    "  iface {} alt {} class=0x{:02x}",
                    iface_desc.interface_number(),
                    iface_desc.setting_number(),
                    iface_desc.class_code(),
                );
                // Only care about HID (class 0x03)
                if iface_desc.class_code() != 0x03 {
                    continue;
                }

                let iface_num = iface_desc.interface_number();
                let mut ep_in = None;
                for ep in iface_desc.endpoint_descriptors() {
                    if ep.direction() == rusb::Direction::In {
                        ep_in = Some(ep.address());
                    }
                }
                let ep_in = match ep_in {
                    Some(e) => e,
                    None => continue,
                };
                println!("  ep_in=0x{ep_in:02x}");

                let handle = device.open().expect("open");
                let _ = handle.set_auto_detach_kernel_driver(true);
                handle.claim_interface(iface_num).expect("claim");
                println!("  claimed interface {iface_num}");

                // Prime with GET_STATUS
                let _ = handle.read_control(
                    0x80,
                    0x00,
                    0,
                    0,
                    &mut [0u8; 2],
                    std::time::Duration::from_millis(100),
                );

                // Try reading HID reports
                println!("  Reading (press buttons on controller)...");
                for i in 0..20 {
                    let mut buf = [0u8; 64];
                    match handle.read_interrupt(
                        ep_in,
                        &mut buf,
                        std::time::Duration::from_millis(500),
                    ) {
                        Ok(n) if n > 0 => {
                            let show: usize = n.min(16);
                            print!("  read {i}: {n}B [");
                            for b in &buf[..show] {
                                print!("{b:02x} ");
                            }
                            println!("]");
                        }
                        Ok(n) => println!("  read {i}: {n}B (empty)"),
                        Err(rusb::Error::Timeout) => println!("  read {i}: timeout"),
                        Err(e) => {
                            println!("  read {i}: {e}");
                            break;
                        }
                    }
                }
                return;
            }
        }
    }
    println!("No 8BitDo device found");
}
