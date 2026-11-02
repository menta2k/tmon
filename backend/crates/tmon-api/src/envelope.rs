//! Response envelope and error mapping (contracts/http-api.md "Envelope").

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use std::collections::BTreeMap;
use tmon_core::ValidationErrors;
use tmon_store::StoreError;

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Meta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub ok: bool,
    pub data: Option<T>,
    pub error: Option<ErrorBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

pub type ApiResult<T> = Result<Json<Envelope<T>>, ApiError>;

pub fn ok<T: Serialize>(data: T) -> ApiResult<T> {
    Ok(Json(Envelope {
        ok: true,
        data: Some(data),
        error: None,
        meta: None,
    }))
}

pub fn ok_meta<T: Serialize>(data: T, meta: Meta) -> ApiResult<T> {
    Ok(Json(Envelope {
        ok: true,
        data: Some(data),
        error: None,
        meta: Some(meta),
    }))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    Unauthorized,
    Validation {
        message: String,
        fields: Option<BTreeMap<String, String>>,
    },
    NotFound,
    Conflict(String),
    UnsupportedFilterRange(Vec<String>),
    RateLimited,
    Internal(String),
}

impl ApiError {
    pub fn validation(message: impl Into<String>) -> Self {
        ApiError::Validation {
            message: message.into(),
            fields: None,
        }
    }

    pub fn field(field: &str, message: &str) -> Self {
        Self::from(ValidationErrors::single(field, message))
    }

    fn parts(&self) -> (StatusCode, &'static str, String) {
        match self {
            ApiError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required".into(),
            ),
            ApiError::Validation { message, .. } => (
                StatusCode::BAD_REQUEST,
                "validation_failed",
                message.clone(),
            ),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource not found".into(),
            ),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m.clone()),
            ApiError::UnsupportedFilterRange(dims) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported_filter_range",
                format!(
                    "this filter combination is only available within raw retention; supported dimensions: {}",
                    dims.join(", ")
                ),
            ),
            ApiError::RateLimited => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many attempts, try again later".into(),
            ),
            ApiError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "internal server error".into(),
            ),
        }
    }
}

impl From<ValidationErrors> for ApiError {
    fn from(e: ValidationErrors) -> Self {
        ApiError::Validation {
            message: e.to_string(),
            fields: Some(e.fields().clone()),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError::NotFound,
            StoreError::Conflict(m) => ApiError::Conflict(m),
            StoreError::Validation(v) => v.into(),
            StoreError::UnsupportedFilterRange { supported_dims } => {
                ApiError::UnsupportedFilterRange(supported_dims)
            }
            other => ApiError::Internal(other.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.parts();
        if let ApiError::Internal(detail) = &self {
            tracing::error!(%detail, "internal error");
        }
        let fields = match self {
            ApiError::Validation { fields, .. } => fields,
            _ => None,
        };
        let body: Envelope<()> = Envelope {
            ok: false,
            data: None,
            error: Some(ErrorBody {
                code,
                message,
                fields,
            }),
            meta: None,
        };
        (status, Json(body)).into_response()
    }
}
