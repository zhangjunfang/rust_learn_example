#![allow(dead_code)]
use std::rc::Rc;

use cookie::{Cookie, CookieJar, Key};
use futures::future::{err as FutErr, ok as FutOk, FutureResult};
use futures::Future;
use time::Duration;

use actix_web::http::header::{self, HeaderValue};
use actix_web::middleware::{Middleware, Response, Started};
use actix_web::{Error, HttpRequest, HttpResponse, Result};

/// Trait provides identity service for the request.
pub trait RequestIdentity {
    /// Return the claimed identity of the user associated request or
    /// ``None`` if no identity can be found associated with the request.
    fn identity(&self) -> Option<String>;

    /// Remember identity.
    fn remember(&self, identity: String);

    /// This method is used to 'forget' the current identity on subsequent
    /// requests.
    fn forget(&self);
}

impl<S> RequestIdentity for HttpRequest<S> {
    fn identity(&self) -> Option<String> {
        if let Some(id) = self.extensions().get::<IdentityBox>() {
            return id.0.identity().map(|s| s.to_owned());
        }
        None
    }

    fn remember(&self, identity: String) {
        if let Some(id) = self.extensions_mut().get_mut::<IdentityBox>() {
            return id.0.as_mut().remember(identity);
        }
    }

    fn forget(&self) {
        if let Some(id) = self.extensions_mut().get_mut::<IdentityBox>() {
            return id.0.forget();
        }
    }
}

/// An identity
pub trait Identity: 'static {
    fn identity(&self) -> Option<&str>;

    fn remember(&mut self, key: String);

    fn forget(&mut self);

    /// Write session to storage backend.
    fn write(&mut self, resp: HttpResponse) -> Result<Response>;
}

/// Identity policy definition.
pub trait IdentityPolicy<S>: Sized + 'static {
    type Identity: Identity;
    type Future: Future<Item = Self::Identity, Error = Error>;

    /// Parse the session from request and load data from a service identity.
    fn from_request(&self, request: &mut HttpRequest<S>) -> Self::Future;
}

/// Middleware that implements identity service
pub struct IdentityService<T> {
    backend: T,
}

impl<T> IdentityService<T> {
    /// Create new identity service with specified backend.
    pub fn new(backend: T) -> Self {
        IdentityService { backend }
    }
}

struct IdentityBox(Box<Identity>);

#[doc(hidden)]
unsafe impl Send for IdentityBox {}
#[doc(hidden)]
unsafe impl Sync for IdentityBox {}

impl<S: 'static, T: IdentityPolicy<S>> Middleware<S> for IdentityService<T> {
    fn start(&self, req: &HttpRequest<S>) -> Result<Started> {
        let mut req = req.clone();

        let fut = self
            .backend
            .from_request(&mut req)
            .then(move |res| match res {
                Ok(id) => {
                    req.extensions_mut().insert(IdentityBox(Box::new(id)));
                    FutOk(None)
                }
                Err(err) => FutErr(err),
            });
        Ok(Started::Future(Box::new(fut)))
    }

    fn response(&self, req: &HttpRequest<S>, resp: HttpResponse) -> Result<Response> {
        if let Some(mut id) = req.extensions_mut().remove::<IdentityBox>() {
            id.0.write(resp)
        } else {
            Ok(Response::Done(resp))
        }
    }
}

/// Identity that uses private cookies as identity storage
pub struct CookieIdentity {
    changed: bool,
    identity: Option<String>,
    inner: Rc<CookieIdentityInner>,
}

impl Identity for CookieIdentity {
    fn identity(&self) -> Option<&str> {
        self.identity.as_ref().map(|s| s.as_ref())
    }

    fn remember(&mut self, value: String) {
        self.changed = true;
        self.identity = Some(value);
    }

    fn forget(&mut self) {
        self.changed = true;
        self.identity = None;
    }

    fn write(&mut self, mut resp: HttpResponse) -> Result<Response> {
        if self.changed {
            let _ = self.inner.set_cookie(&mut resp, self.identity.take());
        }
        Ok(Response::Done(resp))
    }
}

struct CookieIdentityInner {
    key: Key,
    name: String,
    path: String,
    domain: Option<String>,
    secure: bool,
    max_age: Option<Duration>,
}

impl CookieIdentityInner {
    fn new(key: &[u8]) -> CookieIdentityInner {
        CookieIdentityInner {
            key: Key::from_master(key),
            name: "actix-identity".to_owned(),
            path: "/".to_owned(),
            domain: None,
            secure: true,
            max_age: None,
        }
    }

    fn set_cookie(&self, resp: &mut HttpResponse, id: Option<String>) -> Result<()> {
        let some = id.is_some();
        {
            let id = id.unwrap_or_else(|| String::new());
            let mut cookie = Cookie::new(self.name.clone(), id);
            cookie.set_path(self.path.clone());
            cookie.set_secure(self.secure);
            cookie.set_http_only(true);

            if let Some(ref domain) = self.domain {
                cookie.set_domain(domain.clone());
            }

            if let Some(max_age) = self.max_age {
                cookie.set_max_age(max_age);
            }

            let mut jar = CookieJar::new();
            if some {
                jar.private(&self.key).add(cookie);
            } else {
                jar.add_original(cookie.clone());
                jar.private(&self.key).remove(cookie);
            }

            for cookie in jar.delta() {
                let val = HeaderValue::from_str(&cookie.to_string())?;
                resp.headers_mut().append(header::SET_COOKIE, val);
            }
        }

        Ok(())
    }

    fn load<S>(&self, req: &mut HttpRequest<S>) -> Option<String> {
        if let Ok(cookies) = req.cookies() {
            for cookie in cookies.iter() {
                if cookie.name() == self.name {
                    let mut jar = CookieJar::new();
                    jar.add_original(cookie.clone());

                    let cookie_opt = jar.private(&self.key).get(&self.name);
                    if let Some(cookie) = cookie_opt {
                        return Some(cookie.value().into());
                    }
                }
            }
        }
        None
    }
}

/// Use cookies for request identity.
pub struct CookieIdentityPolicy(Rc<CookieIdentityInner>);

impl CookieIdentityPolicy {
    /// Construct new `CookieIdentityPolicy` instance.
    ///
    /// Panics if key length is less than 32 bytes.
    pub fn new(key: &[u8]) -> CookieIdentityPolicy {
        CookieIdentityPolicy(Rc::new(CookieIdentityInner::new(key)))
    }

    /// Sets the `path` field in the session cookie being built.
    pub fn path<S: Into<String>>(mut self, value: S) -> CookieIdentityPolicy {
        Rc::get_mut(&mut self.0).unwrap().path = value.into();
        self
    }

    /// Sets the `name` field in the session cookie being built.
    pub fn name<S: Into<String>>(mut self, value: S) -> CookieIdentityPolicy {
        Rc::get_mut(&mut self.0).unwrap().name = value.into();
        self
    }

    /// Sets the `domain` field in the session cookie being built.
    pub fn domain<S: Into<String>>(mut self, value: S) -> CookieIdentityPolicy {
        Rc::get_mut(&mut self.0).unwrap().domain = Some(value.into());
        self
    }

    /// Sets the `secure` field in the session cookie being built.
    ///
    /// If the `secure` field is set, a cookie will only be transmitted when the
    /// connection is secure - i.e. `https`
    pub fn secure(mut self, value: bool) -> CookieIdentityPolicy {
        Rc::get_mut(&mut self.0).unwrap().secure = value;
        self
    }

    /// Sets the `max-age` field in the session cookie being built.
    pub fn max_age(mut self, value: Duration) -> CookieIdentityPolicy {
        Rc::get_mut(&mut self.0).unwrap().max_age = Some(value);
        self
    }
}

impl<S> IdentityPolicy<S> for CookieIdentityPolicy {
    type Identity = CookieIdentity;
    type Future = FutureResult<CookieIdentity, Error>;

    fn from_request(&self, req: &mut HttpRequest<S>) -> Self::Future {
        let identity = self.0.load(req);
        FutOk(CookieIdentity {
            identity,
            changed: false,
            inner: Rc::clone(&self.0),
        })
    }
}
