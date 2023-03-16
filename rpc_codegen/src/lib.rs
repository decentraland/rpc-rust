//! Generate service code from a service definition.

// Guidelines for generated code:
//
// Use fully-qualified paths, to reduce the chance of clashing with
// user provided names.

use proc_macro2::TokenStream;
use prost_build::{Method, Service, ServiceGenerator};
use quote::{format_ident, quote};

#[derive(Default)]
pub struct RPCServiceGenerator {}

impl RPCServiceGenerator {
    pub fn new() -> RPCServiceGenerator {
        Default::default()
    }

    fn client_stream_request(&self) -> TokenStream {
        quote!(ClientStreamRequest)
    }

    fn server_stream_response(&self) -> TokenStream {
        quote!(ServerStreamResponse)
    }

    fn method_sig_tokens(&self, method: &Method, body: Option<TokenStream>) -> TokenStream {
        let name = format_ident!("{}", method.name);
        let input_type = format_ident!("{}", method.input_type);
        let output_type = format_ident!("{}", method.output_type);

        let input_type = if method.client_streaming {
            let client_stream_request = self.client_stream_request();
            quote!(#client_stream_request<#input_type>)
        } else {
            quote!(#input_type)
        };

        let output_type = if method.server_streaming {
            let server_stream_response = self.server_stream_response();
            quote!(#server_stream_response<#output_type>)
        } else {
            quote!(#output_type)
        };

        if let Some(body) = body {
            quote! {
                async fn #name(&self, request: #input_type)
                    -> #output_type {
                        #body
                    }
            }
        } else {
            quote! {
                async fn #name(&self, request: #input_type)
                    -> #output_type
            }
        }
    }

    fn method_sig_tokens_with_context(&self, method: &Method) -> TokenStream {
        let name = format_ident!("{}", method.name);
        let input_type = format_ident!("{}", method.input_type);
        let output_type = format_ident!("{}", method.output_type);

        let input_type = if method.client_streaming {
            let client_stream_request = self.client_stream_request();
            quote!(#client_stream_request<#input_type>)
        } else {
            quote!(#input_type)
        };

        let output_type = if method.server_streaming {
            let server_stream_response = self.server_stream_response();
            quote!(#server_stream_response<#output_type>)
        } else {
            quote!(#output_type)
        };

        quote! {
            async fn #name(&self, request: #input_type, context: Arc<Context>)
                -> #output_type
        }
    }

    fn generate_stream_types(&self, buf: &mut String) {
        buf.push('\n');
        buf.push_str("use dcl_rpc::stream_protocol::Generator;");
        buf.push('\n');
        buf.push_str("pub type ServerStreamResponse<T> = Generator<T>;");
        buf.push('\n');
        buf.push_str("pub type ClientStreamRequest<T> = Generator<T>;");
        buf.push('\n');
    }

    fn generate_client_trait(&self, service: &Service, buf: &mut String) {
        // This is done with strings rather than tokens because Prost provides functions that
        // return doc comments as strings.
        buf.push('\n');
        service.comments.append_with_indent(0, buf);

        buf.push_str("#[async_trait::async_trait]\n");
        buf.push_str(&format!(
            "pub trait {}: Send + Sync + 'static {{",
            service.name
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!("    {};\n", self.method_sig_tokens(method, None)));
        }
        buf.push_str("}\n");
    }

    fn get_server_service_name(&self, service: &Service) -> String {
        format!("Shared{}", service.name)
    }

    fn generate_server_trait(&self, service: &Service, buf: &mut String) {
        buf.push_str("use std::sync::Arc;\n");
        // This is done with strings rather than tokens because Prost provides functions that
        // return doc comments as strings.
        buf.push('\n');
        service.comments.append_with_indent(0, buf);

        buf.push_str("#[async_trait::async_trait]\n");
        buf.push_str(&format!(
            "pub trait {}<Context>: Send + Sync + 'static {{",
            self.get_server_service_name(service)
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!(
                "    {};\n",
                self.method_sig_tokens_with_context(method)
            ));
        }
        buf.push_str("}\n");
    }

    fn generate_client_service(&self, service: &Service, buf: &mut String) {
        buf.push('\n');
        // Create struct

        buf.push_str("use dcl_rpc::client::{RpcClientModule, ServiceClient};");
        buf.push_str(&format!("pub struct {}Client {{", service.name));
        buf.push_str(&format!("    {},\n", "rpc_client_module: RpcClientModule"));
        buf.push_str("}");

        buf.push('\n');

        buf.push_str(&format!(
            "impl ServiceClient for {}Client {{
    fn set_client_module(rpc_client_module: RpcClientModule) -> Self {{
        Self {{ rpc_client_module }}
    }}
}}
",
            service.name
        ));

        buf.push_str("#[async_trait::async_trait]\n");
        buf.push_str(&format!(
            "impl {} for {}Client {{",
            service.name, service.name
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            let body = match (method.client_streaming, method.server_streaming) {
                (false, false) => self.generate_unary_call(&method.proto_name),
                (false, true) => self.generate_server_streams_procedure(&method.proto_name),
                (true, false) => self.generate_client_streams_procedure(&method.proto_name),
                (true, true) => self.generate_bidir_streams_procedure(&method.proto_name),
            };
            buf.push_str(&format!(
                "    {}\n",
                self.method_sig_tokens(method, Some(body))
            ));
        }
        buf.push_str("}\n");
    }

    fn generate_unary_call(&self, name: &str) -> TokenStream {
        quote! {
            self.rpc_client_module
                .call_unary_procedure(#name, request)
                .await
                .unwrap()
        }
    }

    fn generate_server_streams_procedure(&self, name: &str) -> TokenStream {
        quote! {
            self.rpc_client_module
                .call_server_streams_procedure(#name, request)
                .await
                .unwrap()
        }
    }

    fn generate_client_streams_procedure(&self, name: &str) -> TokenStream {
        quote! {
            self.rpc_client_module
                .call_client_streams_procedure(#name, request)
                .await
                .unwrap()
        }
    }

    fn generate_bidir_streams_procedure(&self, name: &str) -> TokenStream {
        quote! {
            self.rpc_client_module
                .call_bidir_streams_procedure(#name, request)
                .await
                .unwrap()
        }
    }

    fn generate_server_service(&self, service: &Service, buf: &mut String) {
        buf.push_str("use dcl_rpc::server::RpcServerPort;\n");
        buf.push_str("use dcl_rpc::service_module_definition::ServiceModuleDefinition;\n");
        buf.push_str("use prost::Message;\n");

        let name = format!("{}Registration", service.name);
        buf.push('\n');
        buf.push_str(&format!("pub struct {} {{}}\n", name));
        buf.push('\n');

        buf.push('\n');
        buf.push_str(&format!("impl {} {{", name));
        buf.push_str(&format!("    {}", self.generate_register_service(service)));
        buf.push_str("}\n");
    }

    fn generate_register_service(&self, service: &Service) -> TokenStream {
        let service_name = &service.name;
        let name = self.get_server_service_name(service);
        let trait_name: TokenStream = name.parse().unwrap();

        let mut methods: Vec<TokenStream> = vec![];
        for method in &service.methods {
            methods.push(match (method.client_streaming, method.server_streaming) {
                (false, false) => self.generate_add_unary_call(&method),
                (false, true) => self.generate_add_server_streams_procedure(&method),
                (true, false) => self.generate_add_client_streams_procedure(&method),
                (true, true) => self.generate_add_bidir_streams_procedure(&method),
            });
        }
        quote! {
        pub fn register_service<
                S: #trait_name<Context> + Send + Sync + 'static,
                Context: Send + Sync + 'static
            >(
                port: &mut RpcServerPort<Context>,
                service: S
            ) {
                let mut service_def = ServiceModuleDefinition::new();
                // Share service ownership
                let shareable_service = Arc::new(service);

                #(#methods)*

                port.register_module(#service_name.to_string(), service_def)
            }
        }
    }

    fn generate_add_unary_call(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_unary(#proto_method_name, move |request, context| {
                let service = service.clone();
                Box::pin(async move {
                    let response = service
                        .#method_name(#input_type::decode(request.as_slice()).unwrap(), context)
                        .await;
                    response.encode_to_vec()
                })
            });
        }
    }

    fn generate_add_server_streams_procedure(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_server_streams(#proto_method_name, move |request, context| {
                let service = service.clone();
                Box::pin(async move {
                    let server_streams = service
                        .#method_name(#input_type::decode(request.as_slice()).unwrap(), context)
                        .await;
                    // Transforming and filling the new generator is spawned so the response is quick
                    Generator::from_generator(server_streams, |item| item.encode_to_vec())
                })
            });
        }
    }

    fn generate_add_client_streams_procedure(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_client_streams(#proto_method_name, move |request, context| {
                let service = service.clone();
                Box::pin(async move {
                    let generator = Generator::from_generator(request, |item| {
                        #input_type::decode(item.as_slice()).unwrap()
                    });

                    let response = service.#method_name(generator, context).await;
                    response.encode_to_vec()
                })
            });
        }
    }

    fn generate_add_bidir_streams_procedure(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_bidir_streams(#proto_method_name, move |request, context| {
                let service = service.clone();
                Box::pin(async move {
                    let generator = Generator::from_generator(request, |item| {
                        #input_type::decode(item.as_slice()).unwrap()
                    });

                    let response = service.#method_name(generator, context).await;
                    Generator::from_generator(response, |item| item.encode_to_vec())
                })
            });
        }
    }
}

impl ServiceGenerator for RPCServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        self.generate_stream_types(buf);
        self.generate_client_trait(&service, buf);
        self.generate_client_service(&service, buf);
        self.generate_server_trait(&service, buf);
        self.generate_server_service(&service, buf);
        println!("{}", buf.to_string());
    }

    fn finalize(&mut self, _buf: &mut String) {}
}