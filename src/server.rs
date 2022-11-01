use crate::transports::Transport;

pub struct RpcServer {
    transport: Box<dyn Transport>,
    handler: Option<Box<dyn Fn(RpcServerPort)>>,
}

pub struct RpcServerPort {}

impl RpcServer {
    // TODO: allow multiple transports
    pub fn create<T: Transport + 'static>(transport: T) -> Self {
        Self {
            transport: Box::new(transport),
            handler: None,
        }
    }

    pub fn create_port(&self) -> RpcServerPort {
        RpcServerPort {}
    }

    pub fn set_handler<F: Fn(RpcServerPort) + 'static>(&mut self, handler: F) {
        self.handler = Some(Box::new(handler));
    }
}

impl RpcServerPort {
    fn load_module(&self) -> () {}
}
