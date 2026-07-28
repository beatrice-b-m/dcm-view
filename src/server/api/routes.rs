use super::error::{self, ApiError};
use super::handlers;
use super::state::AppState;
use crate::api::contracts::{
    self as api_contracts, ApiEndpointContract, ApiEndpointSpec, ApiMethod, ApiOperation,
    EmbedRoiAnnotations, ErrorResponse, FilesResponse, FrameInfo, HealthResponse, TagNode,
    API_ENDPOINTS, API_PREFIX,
};
use crate::server::web;
use crate::server::RequestActivity;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, put};
use axum::{Json, Router};
#[cfg(feature = "debug-api")]
use tower_http::cors::CorsLayer;

pub(crate) fn router(state: AppState) -> Router {
    let activity = state.activity().clone();
    let api = API_ENDPOINTS
        .iter()
        .fold(Router::new(), register_api_endpoint)
        .fallback(error::not_found_handler)
        .method_not_allowed_fallback(error::method_not_allowed_handler);

    let router = Router::new()
        .route("/", get(web::index))
        .route("/assets/{*path}", get(web::asset))
        .nest(API_PREFIX, api)
        .layer(middleware::from_fn_with_state(
            activity,
            track_request_activity,
        ))
        .with_state(state);

    #[cfg(feature = "debug-api")]
    let router = router.layer(CorsLayer::permissive());

    router
}

async fn track_request_activity(
    State(activity): State<RequestActivity>,
    request: Request,
    next: Next,
) -> Response {
    let _request = activity.request_started();
    next.run(request).await
}

trait HandlerResponseContract {
    type Output;
}

macro_rules! json_handler_response {
    ($($response:ty),+ $(,)?) => {
        $(
            impl HandlerResponseContract for $response {
                type Output = Json<$response>;
            }
        )+
    };
}

json_handler_response!(
    HealthResponse,
    FilesResponse,
    FrameInfo,
    Vec<TagNode>,
    EmbedRoiAnnotations,
);

impl HandlerResponseContract for api_contracts::BlobBody {
    type Output = Response;
}

impl HandlerResponseContract for api_contracts::ArrayBufferBody {
    type Output = Response;
}

trait HandlerErrorContract {
    type Output;
}

impl HandlerErrorContract for ErrorResponse {
    type Output = ApiError;
}

macro_rules! handler_response_type {
    ($spec:ty) => {
        <<$spec as ApiEndpointSpec>::Response as HandlerResponseContract>::Output
    };
}

macro_rules! handler_result_type {
    ($spec:ty) => {
        std::result::Result<
            handler_response_type!($spec),
            <<$spec as ApiEndpointSpec>::Error as HandlerErrorContract>::Output,
        >
    };
}

fn require_endpoint_spec<Spec>(endpoint: &ApiEndpointContract)
where
    Spec: ApiEndpointSpec,
    Spec::Response: HandlerResponseContract,
    Spec::Error: HandlerErrorContract,
{
    assert_eq!(
        *endpoint,
        Spec::CONTRACT,
        "registered endpoint does not match its typed handler specification"
    );
}

fn require_no_query<Spec>()
where
    Spec: ApiEndpointSpec<Query = api_contracts::NoQuery>,
{
}

fn require_no_request<Spec>()
where
    Spec: ApiEndpointSpec<Request = api_contracts::NoRequest>,
{
}

fn register_api_endpoint(
    router: Router<AppState>,
    endpoint: &ApiEndpointContract,
) -> Router<AppState> {
    let require_method = |expected| {
        assert_eq!(
            endpoint.method, expected,
            "API endpoint {} has a method that does not match its handler",
            endpoint.id
        );
    };

    match endpoint.operation {
        ApiOperation::Health => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::Health>(endpoint);
            require_no_query::<api_contracts::Health>();
            require_no_request::<api_contracts::Health>();
            router.route(
                endpoint.path,
                get(|state: State<AppState>| async move {
                    let response: handler_response_type!(api_contracts::Health) =
                        handlers::health(state).await;
                    response
                }),
            )
        }
        ApiOperation::Files => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::Files>(endpoint);
            require_no_query::<api_contracts::Files>();
            require_no_request::<api_contracts::Files>();
            router.route(
                endpoint.path,
                get(|state: State<AppState>| async move {
                    let response: handler_response_type!(api_contracts::Files) =
                        handlers::files(state).await;
                    response
                }),
            )
        }
        ApiOperation::FileInfo => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileInfo>(endpoint);
            require_no_query::<api_contracts::FileInfo>();
            require_no_request::<api_contracts::FileInfo>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>, path: Result<Path<usize>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileInfo) =
                            handlers::info(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileFrame => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileFrame>(endpoint);
            require_no_request::<api_contracts::FileFrame>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: Result<Path<(usize, u32)>, PathRejection>,
                     query: Result<
                        Query<<api_contracts::FileFrame as ApiEndpointSpec>::Query>,
                        QueryRejection,
                    >| async move {
                        let response: handler_result_type!(api_contracts::FileFrame) =
                            handlers::frame(state, path, query).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileRawFrame => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileRawFrame>(endpoint);
            require_no_query::<api_contracts::FileRawFrame>();
            require_no_request::<api_contracts::FileRawFrame>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>,
                     path: Result<Path<(usize, u32)>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileRawFrame) =
                            handlers::raw_frame(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileTags => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileTags>(endpoint);
            require_no_query::<api_contracts::FileTags>();
            require_no_request::<api_contracts::FileTags>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>, path: Result<Path<usize>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileTags) =
                            handlers::tags(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileAnnotationsGet => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::FileAnnotationsGet>(endpoint);
            require_no_query::<api_contracts::FileAnnotationsGet>();
            require_no_request::<api_contracts::FileAnnotationsGet>();
            router.route(
                endpoint.path,
                get(
                    |state: State<AppState>, path: Result<Path<usize>, PathRejection>| async move {
                        let response: handler_result_type!(api_contracts::FileAnnotationsGet) =
                            handlers::annotations(state, path).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::FileAnnotationsUpdate => {
            require_method(ApiMethod::Put);
            require_endpoint_spec::<api_contracts::FileAnnotationsUpdate>(endpoint);
            require_no_query::<api_contracts::FileAnnotationsUpdate>();
            router.route(
                endpoint.path,
                put(
                    |state: State<AppState>,
                     path: Result<Path<usize>, PathRejection>,
                     payload: Result<
                        Json<<api_contracts::FileAnnotationsUpdate as ApiEndpointSpec>::Request>,
                        JsonRejection,
                    >| async move {
                        let response: handler_result_type!(api_contracts::FileAnnotationsUpdate) =
                            handlers::update_annotations(state, path, payload).await;
                        response
                    },
                ),
            )
        }
        ApiOperation::AnnotationsExport => {
            require_method(ApiMethod::Get);
            require_endpoint_spec::<api_contracts::AnnotationsExport>(endpoint);
            require_no_query::<api_contracts::AnnotationsExport>();
            require_no_request::<api_contracts::AnnotationsExport>();
            router.route(
                endpoint.path,
                get(|state: State<AppState>| async move {
                    let response: handler_result_type!(api_contracts::AnnotationsExport) =
                        handlers::export_annotations(state).await;
                    response
                }),
            )
        }
    }
}
