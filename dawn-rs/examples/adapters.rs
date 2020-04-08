use dawn::Instance;

fn main() {
    let instance = Instance::new();
    let adapters = instance.enumerate_adapters();
    for (i, adapter) in adapters.iter().enumerate() {
        let properties = adapter.properties();
        println!("{}) {:?}", i, properties.name);
        println!("    adapter_type: {:?}", properties.adapter_type);
        println!("    backend_type: {:?}", properties.backend_type);
        println!("    vendor_id: {:?}", properties.vendor_id);
        println!("    device_id: {:?}", properties.device_id);
        println!("    extensions: {:#?}", adapter.extensions());
        println!("\n");
    }
}
