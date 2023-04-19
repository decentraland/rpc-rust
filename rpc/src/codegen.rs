//! Generate service code from a service definition in a `.proto` file.

// Guidelines for generated code:
//
// Use fully-qualified paths, to reduce the chance of clashing with
// user provided names.

use proc_macro2::TokenStream;
use prost_build::{Method, Service, ServiceGenerator};
use quote::{format_ident, quote};

#[derive(Default)]
pub struct RPCServiceGenerator {}

pub struct MethodSigTokensParams {
    body: Option<TokenStream>,
    with_context: bool,
    is_for_client: bool,
}

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

    fn method_sig_tokens(&self, method: &Method, params: MethodSigTokensParams) -> TokenStream {
        let input_type = self.extract_input_token(method);
        let output_type = self.extract_output_token(method, params.is_for_client);
        let name = extract_name_token(method);
        let context = extract_context_token(&params);
        let body = extract_body_token(params);

        if let Some(input_type) = input_type {
            quote! {
                async fn #name(&self, request: #input_type #context)
                    #output_type #body
            }
        } else {
            quote! {
                async fn #name(&self #context)
                    #output_type #body
            }
        }
    }

    fn extract_input_token(&self, method: &Method) -> Option<TokenStream> {
        if method.input_type.to_string().eq("()") {
            None
        } else {
            let input_type = format_ident!("{}", method.input_type);
            Some(match method.client_streaming {
                true => {
                    let client_stream_request = self.client_stream_request();
                    quote!(#client_stream_request<#input_type>)
                }
                false => quote!(#input_type),
            })
        }
    }

    fn extract_output_token(&self, method: &Method, is_client: bool) -> TokenStream {
        if method.output_type.to_string().eq("()") {
            // The unit type can not be casted to an Ident, so the empty token is needed
            if is_client {
                quote! { -> ClientResult<()> }
            } else {
                quote! { -> Result<(), Error> }
            }
        } else {
            let output_type = format_ident!("{}", method.output_type);
            match method.server_streaming {
                true => {
                    let server_stream_response = self.server_stream_response();
                    if is_client {
                        quote! {-> ClientResult<#server_stream_response<#output_type>>}
                    } else {
                        quote! {-> Result<#server_stream_response<#output_type>, Error>}
                    }
                }
                false => {
                    if is_client {
                        quote! {-> ClientResult<#output_type>}
                    } else {
                        quote! {-> Result<#output_type, Error>}
                    }
                }
            }
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
        buf.push_str("use dcl_rpc::client::ClientResult;\n");
        buf.push('\n');
        service.comments.append_with_indent(0, buf);

        buf.push_str("#[async_trait::async_trait]\n");
        buf.push_str(&format!(
            "pub trait {}ClientDefinition: Send + Sync + 'static {{",
            service.name
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!(
                "    {};\n",
                self.method_sig_tokens(
                    method,
                    MethodSigTokensParams {
                        body: None,
                        with_context: false,
                        is_for_client: true
                    }
                )
            ));
        }
        buf.push_str("}\n");
    }

    fn get_server_service_name(&self, service: &Service) -> String {
        format!("{}Server", service.name)
    }

    fn generate_server_trait(&self, service: &Service, buf: &mut String) {
        buf.push_str("use std::sync::Arc;\n");
        buf.push_str("use dcl_rpc::rpc_protocol::{RemoteErrorResponse};\n");
        // This is done with strings rather than tokens because Prost provides functions that
        // return doc comments as strings.
        buf.push('\n');
        service.comments.append_with_indent(0, buf);

        buf.push_str("#[async_trait::async_trait]\n");
        buf.push_str(&format!(
            "pub trait {}<Context, Error: RemoteErrorResponse>: Send + Sync + 'static {{",
            self.get_server_service_name(service)
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            buf.push_str(&format!(
                "    {};\n",
                self.method_sig_tokens(
                    method,
                    MethodSigTokensParams {
                        body: None,
                        with_context: true,
                        is_for_client: false
                    }
                )
            ));
        }
        buf.push_str("}\n");
    }

    fn generate_client_service(&self, service: &Service, buf: &mut String) {
        buf.push('\n');
        // Create struct

        buf.push_str(
            "use dcl_rpc::{client::{RpcClientModule, ServiceClient}, transports::Transport};",
        );
        buf.push_str(&format!(
            "pub struct {}Client<T: Transport + 'static> {{",
            service.name
        ));
        buf.push_str(&format!(
            "    {},\n",
            "rpc_client_module: RpcClientModule<T>"
        ));
        buf.push('}');

        buf.push('\n');

        buf.push_str(&format!(
            "impl<T: Transport + 'static> ServiceClient<T> for {}Client<T> {{
    fn set_client_module(rpc_client_module: RpcClientModule<T>) -> Self {{
        Self {{ rpc_client_module }}
    }}
}}
",
            service.name
        ));

        buf.push_str("#[async_trait::async_trait]\n");
        buf.push_str(&format!(
            "impl<T: Transport + 'static> {}ClientDefinition for {}Client<T> {{",
            service.name, service.name
        ));
        for method in service.methods.iter() {
            buf.push('\n');
            method.comments.append_with_indent(1, buf);
            let input_type = self.extract_input_token(method);
            let append_request = input_type.is_some();
            let body = match (method.client_streaming, method.server_streaming) {
                (false, false) => self.generate_unary_call(&method.proto_name, append_request),
                (false, true) => {
                    self.generate_server_streams_procedure(&method.proto_name, append_request)
                }
                (true, false) => {
                    self.generate_client_streams_procedure(&method.proto_name, append_request)
                }
                (true, true) => {
                    self.generate_bidir_streams_procedure(&method.proto_name, append_request)
                }
            };
            buf.push_str(&format!(
                "    {}\n",
                self.method_sig_tokens(
                    method,
                    MethodSigTokensParams {
                        body: Some(body),
                        with_context: false,
                        is_for_client: true
                    }
                )
            ));
        }
        buf.push_str("}\n");
    }

    fn generate_unary_call(&self, name: &str, append_request: bool) -> TokenStream {
        let request = if append_request {
            quote!(request)
        } else {
            quote! { () }
        };
        quote! {
            self.rpc_client_module
                .call_unary_procedure(#name, #request)
                .await
        }
    }

    fn generate_server_streams_procedure(&self, name: &str, append_request: bool) -> TokenStream {
        let request = if append_request {
            quote!(request)
        } else {
            quote! { () }
        };

        quote! {
            self.rpc_client_module
                .call_server_streams_procedure(#name, #request)
                .await
        }
    }

    fn generate_client_streams_procedure(&self, name: &str, append_request: bool) -> TokenStream {
        let request = if append_request {
            quote!(request)
        } else {
            quote! { () }
        };

        quote! {
            self.rpc_client_module
                .call_client_streams_procedure(#name, #request)
                .await
        }
    }

    fn generate_bidir_streams_procedure(&self, name: &str, append_request: bool) -> TokenStream {
        let request = if append_request {
            quote!(request)
        } else {
            quote! { () }
        };

        quote! {
            self.rpc_client_module
                .call_bidir_streams_procedure(#name, #request)
                .await
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
                (false, false) => self.generate_add_unary_call(method),
                (false, true) => self.generate_add_server_streams_procedure(method),
                (true, false) => self.generate_add_client_streams_procedure(method),
                (true, true) => self.generate_add_bidir_streams_procedure(method),
            });
        }
        quote! {
        pub fn register_service<
                S: #trait_name<Context, Error> + Send + Sync + 'static,
                Context: Send + Sync + 'static,
                Error: RemoteErrorResponse + Send + Sync + 'static
            >(
                port: &mut RpcServerPort<Context>,
                service: S
            ) {
                let mut service_def = ServiceModuleDefinition::new();
                // Share service ownership
                let shareable_service = Arc::new(service);

                #(#methods)*

                port.register_module(#service_name, service_def)
            }
        }
    }

    fn generate_add_unary_call(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type = self.extract_input_token(method);

        let service_call;
        let request;
        if let Some(input_type) = input_type {
            service_call = quote! {
                service.#method_name(#input_type::decode(request.as_slice()).unwrap(), context).await
            };
            request = quote! {request}
        } else {
            service_call = quote! { service.#method_name(context).await };
            request = quote! {_request}
        };
        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_unary(#proto_method_name, move |#request, context| {
                let service = service.clone();
                Box::pin(async move {
                    match #service_call {
                        Ok(response) => Ok(response.encode_to_vec()),
                        Err(err) => Err(err.into())
                    }
                })
            });
        }
    }

    fn generate_add_server_streams_procedure(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        let extracted_input_type = self.extract_input_token(method);

        let service_stream;
        let request;
        if extracted_input_type.is_some() {
            service_stream = quote! {
                service.#method_name(#input_type::decode(request.as_slice()).unwrap(), context).await
            };
            request = quote! { request };
        } else {
            service_stream = quote! {
                service.#method_name(context).await;
            };
            request = quote! { _request };
        };

        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_server_streams(#proto_method_name, move |#request, context| {
                let service = service.clone();
                Box::pin(async move {
                    match #service_stream {
                        // Transforming and filling the new generator is spawned so the response is quick
                        Ok(server_streams_generator) => Ok(Generator::from_generator(server_streams_generator, |item| Some(item.encode_to_vec()))),
                        Err(err) => Err(err.into())
                    }
                })
            });
        }
    }

    fn generate_add_client_streams_procedure(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        let extracted_input_type = self.extract_input_token(method);

        let input;
        let request;
        if extracted_input_type.is_some() {
            input = quote! {
                #input_type::decode(item.as_slice()).unwrap()
            };
            request = quote! { request };
        } else {
            input = quote! { () };
            request = quote! { _request };
        };
        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_client_streams(#proto_method_name, move |#request, context| {
                let service = service.clone();
                Box::pin(async move {
                    let generator = Generator::from_generator(request, |item| {
                        Some(#input)
                    });

                    match service.#method_name(generator, context).await {
                        Ok(response) => Ok(response.encode_to_vec()),
                        Err(err) => Err(err.into())
                    }
                })
            });
        }
    }

    fn generate_add_bidir_streams_procedure(&self, method: &Method) -> TokenStream {
        let method_name: TokenStream = method.name.parse().unwrap();
        let proto_method_name = &method.proto_name;
        let input_type: TokenStream = method.input_type.parse().unwrap();
        let extracted_input_type = self.extract_input_token(method);

        let input;
        let request;
        if extracted_input_type.is_some() {
            input = quote! {
                #input_type::decode(item.as_slice()).unwrap()
            };
            request = quote! { request };
        } else {
            input = quote! { () };
            request = quote! { _request };
        };

        quote! {
            let service = Arc::clone(&shareable_service);
            service_def.add_bidir_streams(#proto_method_name, move |#request, context| {
                let service = service.clone();
                Box::pin(async move {
                    let generator = Generator::from_generator(request, |item| {
                        Some(#input)
                    });

                    match service.#method_name(generator, context).await {
                        Ok(response_generator) => Ok(Generator::from_generator(response_generator, |item| Some(item.encode_to_vec()))),
                        Err(err) => Err(err.into())
                    }
                })
            });
        }
    }
}

fn extract_name_token(method: &Method) -> proc_macro2::Ident {
    format_ident!("{}", method.name)
}

fn extract_context_token(params: &MethodSigTokensParams) -> TokenStream {
    match params.with_context {
        true => quote! {, context: Arc<Context>},
        false => TokenStream::default(),
    }
}

fn extract_body_token(params: MethodSigTokensParams) -> TokenStream {
    let body = params.body;
    match body {
        Some(body) => quote! { { #body } },
        None => TokenStream::default(),
    }
}

impl ServiceGenerator for RPCServiceGenerator {
    fn generate(&mut self, service: Service, buf: &mut String) {
        self.generate_stream_types(buf);
        self.generate_client_trait(&service, buf);
        self.generate_client_service(&service, buf);
        self.generate_server_trait(&service, buf);
        self.generate_server_service(&service, buf);
        println!("{}", buf);
    }

    fn finalize(&mut self, _buf: &mut String) {}
}
