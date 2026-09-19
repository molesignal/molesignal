// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Route- and resource-level IAM permission attributes for MoleSignal HTTP
//! handlers.
//!
//! The macro deliberately performs only the coarse capability check already
//! backed by the database-resolved `IamContext`. Permission definitions,
//! scopes, feature gates, and role assignments live in `iam_permissions` and
//! the IAM role tables; this crate does not maintain a Rust permission
//! catalog. The resource macro loads the canonical resource first and then
//! delegates ownership, cross-organization grants, and target scoping to the
//! runtime IAM engine.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Expr, FnArg, GenericArgument, Ident, ItemFn, LitStr, Pat, PathArguments, Stmt, Token, Type,
    parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote_spanned,
    punctuated::Punctuated,
    spanned::Spanned,
};

/// Require one database-catalog IAM permission before entering an HTTP handler.
///
/// The annotated function must extract an `IamContext` (or compatibility
/// `AuthContext`) through axum's `Extension`. The binding name is discovered
/// from the signature, so both `ctx` and `context` work:
///
/// ```ignore
/// #[permission("dashboards.read")]
/// async fn get_dashboard(
///     Extension(context): Extension<IamContext>,
/// ) -> Result<Json<Dashboard>> {
///     // Resource-level authorization remains explicit after loading.
/// }
/// ```
///
/// A system/organization compatibility route may accept more than one
/// database permission without introducing a code-side permission enum:
///
/// ```ignore
/// #[permission(any("streams.query", "sys.telemetry.read"))]
/// async fn query(
///     Extension(context): Extension<IamContext>,
/// ) -> Result<Json<QueryResult>> {
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn permission(attr: TokenStream, item: TokenStream) -> TokenStream {
    let requirement = parse_macro_input!(attr as PermissionRequirement);
    let function = parse_macro_input!(item as ItemFn);

    match expand_permission(requirement, function) {
        Ok(expanded) => expanded.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Load and authorize one protected resource before entering an HTTP handler.
///
/// The resource type implements the application's `ProtectedResource` trait.
/// The macro discovers the `State<AppState>` and `Extension<IamContext>`
/// bindings, loads the resource exactly once, checks its real organization and
/// resource ID, and binds the authorized value for the business body:
///
/// ```ignore
/// #[resource_permission(
///     action = "dashboards.read",
///     resource = Dashboard,
///     id = Id::from_string(id),
///     bind = dashboard
/// )]
/// async fn get_dashboard(
///     State(state): State<AppState>,
///     Extension(context): Extension<IamContext>,
///     Path(id): Path<String>,
/// ) -> Result<Json<Dashboard>> {
///     Ok(Json(dashboard))
/// }
/// ```
#[proc_macro_attribute]
pub fn resource_permission(attr: TokenStream, item: TokenStream) -> TokenStream {
    let arguments = parse_macro_input!(attr as ResourcePermissionArgs);
    let function = parse_macro_input!(item as ItemFn);

    match expand_resource_permission(arguments, function) {
        Ok(expanded) => expanded.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

enum PermissionRequirement {
    One(LitStr),
    Any(Vec<LitStr>),
}

impl Parse for PermissionRequirement {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(LitStr) {
            return input.parse().map(Self::One);
        }
        let operator: Ident = input.parse()?;
        if operator != "any" {
            return Err(syn::Error::new(
                operator.span(),
                "expected a permission string or any(\"key\", ...)",
            ));
        }
        let content;
        parenthesized!(content in input);
        let permissions = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?
            .into_iter()
            .collect();
        Ok(Self::Any(permissions))
    }
}

enum ResourceAction {
    Permission(PermissionRequirement),
    Dynamic(Expr),
    Resolve(Expr),
    ResolveAll(Expr),
}

impl Parse for ResourceAction {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if input.peek(Ident) {
            let fork = input.fork();
            let operator: Ident = fork.parse()?;
            if operator == "dynamic" || operator == "resolve" || operator == "resolve_all" {
                let operator: Ident = input.parse()?;
                let content;
                parenthesized!(content in input);
                let expression = content.parse()?;
                return Ok(match operator.to_string().as_str() {
                    "dynamic" => Self::Dynamic(expression),
                    "resolve" => Self::Resolve(expression),
                    "resolve_all" => Self::ResolveAll(expression),
                    _ => unreachable!("validated resource action operator"),
                });
            }
        }
        input.parse().map(ResourceAction::Permission)
    }
}

struct ResourcePermissionArgs {
    action: ResourceAction,
    resource: Type,
    id: Expr,
    bind: Ident,
}

impl Parse for ResourcePermissionArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut action = None;
        let mut resource = None;
        let mut id = None;
        let mut bind = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "action" => set_once(&mut action, input.parse()?, &key)?,
                "resource" => set_once(&mut resource, input.parse()?, &key)?,
                "id" => set_once(&mut id, input.parse()?, &key)?,
                "bind" => set_once(&mut bind, input.parse()?, &key)?,
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "expected action, resource, id, or bind",
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            action: action.ok_or_else(|| syn::Error::new(input.span(), "missing action"))?,
            resource: resource.ok_or_else(|| syn::Error::new(input.span(), "missing resource"))?,
            id: id.ok_or_else(|| syn::Error::new(input.span(), "missing id"))?,
            bind: bind.ok_or_else(|| syn::Error::new(input.span(), "missing bind"))?,
        })
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &Ident) -> syn::Result<()> {
    if slot.replace(value).is_some() {
        Err(syn::Error::new(
            key.span(),
            format!("duplicate resource_permission argument `{key}`"),
        ))
    } else {
        Ok(())
    }
}

fn expand_permission(
    requirement: PermissionRequirement,
    mut function: ItemFn,
) -> syn::Result<TokenStream2> {
    let context = auth_context_binding(&function)?;
    let check: Stmt = match requirement {
        PermissionRequirement::One(permission) => {
            validate_permission(&permission)?;
            parse_quote_spanned! {permission.span()=>
                crate::api::http::middleware::Permission::require_key(&#context, #permission)?;
            }
        }
        PermissionRequirement::Any(permissions) => {
            let Some(first) = permissions.first() else {
                return Err(syn::Error::new(
                    function.sig.ident.span(),
                    "any(...) requires at least one permission",
                ));
            };
            for permission in &permissions {
                validate_permission(permission)?;
            }
            parse_quote_spanned! {first.span()=>
                crate::api::http::middleware::Permission::require_any_key(
                    &#context,
                    &[#(#permissions),*],
                )?;
            }
        }
    };
    function.block.stmts.insert(0, check);
    Ok(quote!(#function))
}

fn expand_resource_permission(
    arguments: ResourcePermissionArgs,
    mut function: ItemFn,
) -> syn::Result<TokenStream2> {
    let context = auth_context_binding(&function)?;
    let state = app_state_binding(&function)?;
    let ResourcePermissionArgs {
        action,
        resource,
        id,
        bind,
    } = arguments;
    let load: Stmt = match action {
        ResourceAction::Permission(PermissionRequirement::One(permission)) => {
            validate_permission(&permission)?;
            parse_quote_spanned! {permission.span()=>
                let #bind: #resource =
                    crate::api::http::middleware::authorize_resource::<#resource>(
                        &#state,
                        &#context,
                        #id,
                        #permission,
                    )
                    .await?;
            }
        }
        ResourceAction::Permission(PermissionRequirement::Any(permissions)) => {
            let Some(first) = permissions.first() else {
                return Err(syn::Error::new(
                    function.sig.ident.span(),
                    "any(...) requires at least one permission",
                ));
            };
            for permission in &permissions {
                validate_permission(permission)?;
            }
            parse_quote_spanned! {first.span()=>
                let #bind: #resource =
                    crate::api::http::middleware::authorize_resource_any::<#resource>(
                        &#state,
                        &#context,
                        #id,
                        &[#(#permissions),*],
                    )
                .await?;
            }
        }
        ResourceAction::Dynamic(permission) => {
            parse_quote_spanned! {permission.span()=>
                let #bind: #resource = {
                    let __molesignal_permission = #permission;
                    crate::api::http::middleware::authorize_resource::<#resource>(
                        &#state,
                        &#context,
                        #id,
                        &*__molesignal_permission,
                    )
                    .await?
                };
            }
        }
        ResourceAction::Resolve(resolver) => {
            parse_quote_spanned! {resolver.span()=>
                let #bind: #resource =
                    crate::api::http::middleware::authorize_resource_with::<#resource, _>(
                        &#state,
                        &#context,
                        #id,
                        #resolver,
                    )
                    .await?;
            }
        }
        ResourceAction::ResolveAll(resolver) => {
            parse_quote_spanned! {resolver.span()=>
                let #bind: #resource =
                    crate::api::http::middleware::authorize_resource_all_with::<#resource, _>(
                        &#state,
                        &#context,
                        #id,
                        #resolver,
                    )
                    .await?;
            }
        }
    };
    function.block.stmts.insert(0, load);
    Ok(quote!(#function))
}

fn validate_permission(permission: &LitStr) -> syn::Result<()> {
    let value = permission.value();
    if value.is_empty() {
        return Err(syn::Error::new(
            permission.span(),
            "permission must not be empty",
        ));
    }
    if !value.contains('.')
        || value.split('.').any(str::is_empty)
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return Err(syn::Error::new(
            permission.span(),
            "permission must use the canonical lowercase key format",
        ));
    }
    Ok(())
}

fn auth_context_binding(function: &ItemFn) -> syn::Result<Ident> {
    let mut binding = None;
    for input in &function.sig.inputs {
        let FnArg::Typed(argument) = input else {
            continue;
        };
        if !is_auth_extension(&argument.ty) {
            continue;
        }
        let Pat::TupleStruct(extractor) = argument.pat.as_ref() else {
            return Err(syn::Error::new(
                argument.pat.span(),
                "permission handler must bind Extension<IamContext>",
            ));
        };
        let Some(Pat::Ident(ident)) = extractor.elems.first() else {
            return Err(syn::Error::new(
                extractor.span(),
                "permission handler must bind Extension<IamContext> to an identifier",
            ));
        };
        if binding.replace(ident.ident.clone()).is_some() {
            return Err(syn::Error::new(
                argument.span(),
                "permission handler has more than one authentication context",
            ));
        }
    }
    binding.ok_or_else(|| {
        syn::Error::new(
            function.sig.ident.span(),
            "permission handler requires Extension<IamContext>",
        )
    })
}

fn app_state_binding(function: &ItemFn) -> syn::Result<Ident> {
    let mut binding = None;
    for input in &function.sig.inputs {
        let FnArg::Typed(argument) = input else {
            continue;
        };
        if !is_app_state_extractor(&argument.ty) {
            continue;
        }
        let Pat::TupleStruct(extractor) = argument.pat.as_ref() else {
            return Err(syn::Error::new(
                argument.pat.span(),
                "resource permission handler must bind State<AppState>",
            ));
        };
        let Some(Pat::Ident(ident)) = extractor.elems.first() else {
            return Err(syn::Error::new(
                extractor.span(),
                "resource permission handler must bind State<AppState> to an identifier",
            ));
        };
        if binding.replace(ident.ident.clone()).is_some() {
            return Err(syn::Error::new(
                argument.span(),
                "resource permission handler has more than one application state",
            ));
        }
    }
    binding.ok_or_else(|| {
        syn::Error::new(
            function.sig.ident.span(),
            "resource permission handler requires State<AppState>",
        )
    })
}

fn is_auth_extension(ty: &Type) -> bool {
    is_extractor_with_inner(ty, "Extension", &["IamContext", "AuthContext"])
}

fn is_app_state_extractor(ty: &Type) -> bool {
    is_extractor_with_inner(ty, "State", &["AppState"])
}

fn is_extractor_with_inner(ty: &Type, extractor_name: &str, inner_names: &[&str]) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    let Some(extension) = type_path.path.segments.last() else {
        return false;
    };
    if extension.ident != extractor_name {
        return false;
    }
    let PathArguments::AngleBracketed(arguments) = &extension.arguments else {
        return false;
    };
    arguments.args.iter().any(|argument| {
        let GenericArgument::Type(Type::Path(inner)) = argument else {
            return false;
        };
        inner
            .path
            .segments
            .last()
            .is_some_and(|segment| inner_names.iter().any(|name| segment.ident == *name))
    })
}

#[cfg(test)]
mod tests {
    use syn::{ItemFn, LitStr, parse_quote, parse_str};

    use super::{
        PermissionRequirement, ResourcePermissionArgs, expand_permission,
        expand_resource_permission,
    };

    #[test]
    fn expands_against_discovered_context_binding() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                axum::Extension(context): axum::Extension<crate::app::iam::IamContext>,
            ) -> crate::shared::Result<()> {
                Ok(())
            }
        };
        let permission: LitStr = parse_quote!("dashboards.read");

        let expanded = expand_permission(PermissionRequirement::One(permission), function)
            .expect("expand permission")
            .to_string();

        assert!(expanded.contains("Permission :: require_key (& context , \"dashboards.read\")"));
    }

    #[test]
    fn expands_any_permission_requirement() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                Extension(ctx): Extension<IamContext>,
            ) -> crate::shared::Result<()> {
                Ok(())
            }
        };

        let expanded = expand_permission(
            PermissionRequirement::Any(vec![
                parse_quote!("streams.query"),
                parse_quote!("sys.telemetry.read"),
            ]),
            function,
        )
        .expect("expand any permission")
        .to_string();

        assert!(expanded.contains("Permission :: require_any_key"));
        assert!(expanded.contains("\"streams.query\""));
        assert!(expanded.contains("\"sys.telemetry.read\""));
    }

    #[test]
    fn resource_permission_loads_and_binds_authorized_resource() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                State(app): State<AppState>,
                Extension(ctx): Extension<IamContext>,
                Path(id): Path<String>,
            ) -> crate::shared::Result<Dashboard> {
                Ok(dashboard)
            }
        };
        let arguments: ResourcePermissionArgs = parse_str(
            r#"action = "dashboards.read",
               resource = Dashboard,
               id = Id::from_string(id),
               bind = dashboard"#,
        )
        .expect("parse resource permission");

        let expanded = expand_resource_permission(arguments, function)
            .expect("expand resource permission")
            .to_string();

        assert!(expanded.contains("authorize_resource :: < Dashboard >"));
        assert!(expanded.contains("& app"));
        assert!(expanded.contains("& ctx"));
        assert!(expanded.contains("let dashboard : Dashboard"));
        assert!(expanded.contains("\"dashboards.read\""));
    }

    #[test]
    fn resource_permission_supports_any_database_action() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                State(state): State<AppState>,
                Extension(context): Extension<IamContext>,
                Path(id): Path<Id>,
            ) -> crate::shared::Result<StreamDefinition> {
                Ok(stream)
            }
        };
        let arguments: ResourcePermissionArgs = parse_str(
            r#"action = any("streams.read", "sys.telemetry.read"),
               resource = StreamDefinition,
               id = id,
               bind = stream"#,
        )
        .expect("parse resource permission");

        let expanded = expand_resource_permission(arguments, function)
            .expect("expand resource permission")
            .to_string();

        assert!(expanded.contains("authorize_resource_any :: < StreamDefinition >"));
        assert!(expanded.contains("\"streams.read\""));
        assert!(expanded.contains("\"sys.telemetry.read\""));
    }

    #[test]
    fn resource_permission_supports_loaded_resource_action_resolver() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                State(state): State<AppState>,
                Extension(context): Extension<IamContext>,
                Path(id): Path<Id>,
            ) -> crate::shared::Result<ResourceShare> {
                Ok(share)
            }
        };
        let arguments: ResourcePermissionArgs = parse_str(
            r#"action = resolve(share_permission),
               resource = ResourceShare,
               id = id,
               bind = share"#,
        )
        .expect("parse resolved resource permission");

        let expanded = expand_resource_permission(arguments, function)
            .expect("expand resolved resource permission")
            .to_string();

        assert!(expanded.contains("authorize_resource_with :: < ResourceShare"));
        assert!(expanded.contains("share_permission"));
    }

    #[test]
    fn resource_permission_supports_request_derived_action() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                State(state): State<AppState>,
                Extension(context): Extension<IamContext>,
                Json(request): Json<CreateShareRequest>,
            ) -> crate::shared::Result<ShareableResource> {
                Ok(resource)
            }
        };
        let arguments: ResourcePermissionArgs = parse_str(
            r#"action = dynamic(share_permission(&request.resource_type)?),
               resource = ShareableResource,
               id = ShareableResourceId::new(&request.resource_type, &request.resource_id)?,
               bind = resource"#,
        )
        .expect("parse dynamic resource permission");

        let expanded = expand_resource_permission(arguments, function)
            .expect("expand dynamic resource permission")
            .to_string();

        assert!(expanded.contains("authorize_resource :: < ShareableResource >"));
        assert!(expanded.contains("__molesignal_permission"));
        assert!(expanded.contains("share_permission"));
    }

    #[test]
    fn resource_permission_supports_all_loaded_resource_actions() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                State(state): State<AppState>,
                Extension(context): Extension<IamContext>,
                Path(id): Path<Id>,
            ) -> crate::shared::Result<ScheduledPipeline> {
                Ok(pipeline)
            }
        };
        let arguments: ResourcePermissionArgs = parse_str(
            r#"action = resolve_all(|pipeline| update_permissions(pipeline, &request)),
               resource = ScheduledPipeline,
               id = id,
               bind = pipeline"#,
        )
        .expect("parse resolved resource permissions");

        let expanded = expand_resource_permission(arguments, function)
            .expect("expand resolved resource permissions")
            .to_string();

        assert!(expanded.contains("authorize_resource_all_with :: < ScheduledPipeline"));
        assert!(expanded.contains("update_permissions"));
    }

    #[test]
    fn rejects_handler_without_auth_context() {
        let function: ItemFn = parse_quote! {
            async fn handler() -> crate::shared::Result<()> {
                Ok(())
            }
        };
        let permission: LitStr = parse_quote!("dashboards.read");

        let error = expand_permission(PermissionRequirement::One(permission), function)
            .expect_err("missing context");

        assert!(error.to_string().contains("requires Extension<IamContext>"));
    }

    #[test]
    fn rejects_noncanonical_permission_key() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                Extension(ctx): Extension<IamContext>,
            ) -> crate::shared::Result<()> {
                Ok(())
            }
        };
        let permission: LitStr = parse_quote!("Dashboard:Read");

        let error = expand_permission(PermissionRequirement::One(permission), function)
            .expect_err("invalid key");

        assert!(error.to_string().contains("canonical lowercase key format"));
    }

    #[test]
    fn rejects_permission_key_without_domain_segment() {
        let function: ItemFn = parse_quote! {
            async fn handler(
                Extension(ctx): Extension<IamContext>,
            ) -> crate::shared::Result<()> {
                Ok(())
            }
        };
        let permission: LitStr = parse_quote!("read");

        let error = expand_permission(PermissionRequirement::One(permission), function)
            .expect_err("invalid key");

        assert!(error.to_string().contains("canonical lowercase key format"));
    }
}
