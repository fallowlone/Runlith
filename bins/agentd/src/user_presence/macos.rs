use std::sync::mpsc;

use block2::RcBlock;
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_foundation::NSString;

use super::{Error, TIMEOUT};

#[link(name = "LocalAuthentication", kind = "framework")]
unsafe extern "C" {}

const DEVICE_OWNER_AUTHENTICATION: isize = 2;

pub(super) fn authenticate(reason: &str) -> Result<(), Error> {
    evaluate(reason, true)
}

fn evaluate(reason: &str, interactive: bool) -> Result<(), Error> {
    if reason.trim().is_empty() {
        return Err(Error::Unavailable);
    }
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: LAContext is linked from Apple's framework. Selectors and BOOL/NSInteger
        // signatures match LAContext.h; the retained context stays on this thread until invalidate.
        let context: Retained<AnyObject> = unsafe { msg_send![class!(LAContext), new] };
        unsafe {
            let _: () =
                msg_send![&*context, setTouchIDAuthenticationAllowableReuseDuration: 0.0f64];
            let _: () = msg_send![&*context, setInteractionNotAllowed: Bool::new(!interactive)];
        }
        let mut failure: *mut AnyObject = std::ptr::null_mut();
        let available: Bool = unsafe {
            msg_send![&*context, canEvaluatePolicy: DEVICE_OWNER_AUTHENTICATION, error: &mut failure]
        };
        if !available.as_bool() {
            if !failure.is_null() {
                // SAFETY: the API supplied an autoreleased NSError valid inside this pool.
                let code: isize = unsafe { msg_send![failure, code] };
                eprintln!("agentd: OS user presence unavailable (LocalAuthentication code {code})");
            }
            return Err(Error::Unavailable);
        }
        let message = NSString::from_str(reason);
        let (tx, rx) = mpsc::sync_channel(1);
        let reply = RcBlock::new(move |success: Bool, _error: *mut AnyObject| {
            let _ = tx.try_send(success.as_bool());
        });
        // SAFETY: the framework copies the escaping block. It owns only a Send channel;
        // no Objective-C argument or thread-bound reference escapes the callback.
        unsafe {
            let _: () = msg_send![&*context, evaluatePolicy: DEVICE_OWNER_AUTHENTICATION, localizedReason: &*message, reply: &*reply];
        }
        let result = match rx.recv_timeout(TIMEOUT) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Error::Denied),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::TimedOut),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Unavailable),
        };
        // SAFETY: context is still retained; invalidation cancels a pending evaluation.
        // A callback arriving after timeout has no authority to perform an operation.
        unsafe {
            let _: () = msg_send![&*context, invalidate];
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noninteractive_context_cannot_authorize_an_operation() {
        assert!(evaluate("verify Runlith headless refusal", false).is_err());
    }

    #[test]
    #[ignore = "requires a real system authentication dialog and the operator"]
    fn authenticates_with_system_user_presence() {
        assert_eq!(
            std::env::var("RUNLITH_USER_PRESENCE_SMOKE").as_deref(),
            Ok("1")
        );
        assert_eq!(
            authenticate("verify Runlith operation confirmation"),
            Ok(())
        );
    }
}
