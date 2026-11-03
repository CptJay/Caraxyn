//! The outbox: offers wait here until their recipient can be reached.
//!
//! A background task, the postman, sends whatever is due, all at once. It
//! retries with growing pauses, and is woken early at startup, by a new share
//! and when a recipient connects to us.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::sync::{Notify, broadcast, watch};
use tracing::{info, warn};

use super::{
    Node, Timing,
    control::{ControlError, ControlMsg},
    db::{Mail, OutboxEntry},
};
use crate::{
    contacts::now_ms,
    error::Error,
    events::{Event, tell, warn_about},
    ids::{ContentHash, PeerId},
    shares::PendingOffer,
};

/// Marks one message as being sent. When it is dropped, at the end of its
/// delivery task or when that task panics, the message may be tried again and
/// the postman recomputes when the next one is due.
struct OnItsWay {
    key: (PeerId, ContentHash),
    in_flight: Arc<Mutex<HashSet<(PeerId, ContentHash)>>>,
    postman: Arc<Notify>,
}

impl Drop for OnItsWay {
    fn drop(&mut self) {
        // A poisoned lock still holds a usable set: never leave the key stuck.
        let mut in_flight = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        in_flight.remove(&self.key);
        self.postman.notify_one();
    }
}

/// Runs `task` again whenever it panics, after `pause`, until it ends normally.
/// A panic is reported as a warning event.
pub(super) async fn supervise<F, Fut>(
    events: broadcast::Sender<Event>,
    name: &'static str,
    pause: Duration,
    mut task: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    loop {
        match tokio::spawn(task()).await {
            Ok(()) => return,
            Err(e) if e.is_panic() => {
                warn_about(&events, name, "it crashed and is restarted");
                tokio::time::sleep(pause).await;
            }
            // Cancelled: the runtime is shutting down.
            Err(_) => return,
        }
    }
}

/// What came of one delivery attempt.
enum Delivery {
    /// The offer is in the recipient's inbox.
    Delivered,
    /// The recipient said no, or the share no longer exists: never retry.
    Dropped,
    /// Not reached (offline, network trouble): try again later.
    Failed,
}

/// Pause before the next try after `attempts` failures: the first pause,
/// doubled each time, never more than the longest.
pub(super) fn retry_delay(attempts: u32, timing: &Timing) -> Duration {
    let doublings = attempts.saturating_sub(1).min(16);
    timing
        .retry_first
        .saturating_mul(1 << doublings)
        .min(timing.retry_max)
}

impl Node {
    /// Every offer still waiting to reach its recipient.
    ///
    /// # Errors
    ///
    /// [`Error::Database`] if the outbox cannot be read.
    pub fn pending_offers(&self) -> Result<Vec<PendingOffer>, Error> {
        Ok(self
            .access
            .db()
            .outbox()?
            .into_iter()
            .map(|entry| PendingOffer {
                to: entry.to,
                root: entry.root,
                revoke: entry.mail == Mail::Revoke,
                attempts: entry.attempts,
            })
            .collect())
    }

    /// Runs until `stop` changes: starts sending what is due, then sleeps until
    /// the next offer is due or something wakes it (a new share, a recipient
    /// connecting, a delivery finishing).
    pub(super) async fn run_postman(self, mut stop: watch::Receiver<bool>) {
        // Offers being sent right now, so a slow one is never sent twice at once.
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        // At startup every waiting offer is tried at once, whatever its schedule.
        let mut everything = true;
        loop {
            let wait = self.send_due(everything, &in_flight).unwrap_or_else(|e| {
                warn_about(&self.events, "reading the outbox", e);
                self.timing.retry_max
            });
            everything = false;
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.postman.notified() => {}
                _ = stop.changed() => return,
            }
        }
    }

    /// Starts sending every due offer that is not already on its way; each one
    /// records its own result, so a slow recipient holds up nobody else.
    /// Returns how long until the next waiting offer is due.
    fn send_due(
        &self,
        everything: bool,
        in_flight: &Arc<Mutex<HashSet<(PeerId, ContentHash)>>>,
    ) -> Result<Duration, Error> {
        let now = now_ms();
        let mut next: Option<u64> = None;
        for entry in self.access.db().outbox()? {
            let key = (entry.to, entry.root);
            if in_flight
                .lock()
                .expect("outbox lock poisoned")
                .contains(&key)
            {
                continue;
            }
            if !everything && entry.next_try > now {
                next = Some(next.map_or(entry.next_try, |n| n.min(entry.next_try)));
                continue;
            }
            in_flight.lock().expect("outbox lock poisoned").insert(key);
            let on_its_way = OnItsWay {
                key,
                in_flight: Arc::clone(in_flight),
                postman: Arc::clone(&self.postman),
            };
            let node = self.clone();
            tokio::spawn(async move {
                // Dropped at the end of this task, even if it panics.
                let _on_its_way = on_its_way;
                let delivery = node.deliver(entry).await;
                if let Err(e) = node.record(entry, delivery) {
                    warn_about(&node.events, "updating the outbox", e);
                }
            });
        }
        Ok(next.map_or(self.timing.retry_max, |at| {
            Duration::from_millis(at.saturating_sub(now))
        }))
    }

    /// Updates the outbox after one attempt.
    fn record(&self, entry: OutboxEntry, delivery: Delivery) -> Result<(), Error> {
        let (to, root, mail) = (entry.to, entry.root, entry.mail);
        let revoke = mail == Mail::Revoke;
        match delivery {
            Delivery::Delivered => {
                info!(to = %to.short(), share = %root.short(), ?mail, "delivered");
                self.access.db().remove_mail(to, root, mail)?;
                tell(&self.events, Event::Delivered { to, root, revoke });
                Ok(())
            }
            Delivery::Dropped => {
                warn!(to = %to.short(), share = %root.short(), ?mail, "refused, not retrying");
                self.access.db().remove_mail(to, root, mail)?;
                tell(&self.events, Event::Undeliverable { to, root, revoke });
                Ok(())
            }
            Delivery::Failed => {
                let attempts = entry.attempts.saturating_add(1);
                let pause = retry_delay(attempts, &self.timing);
                let pause = u64::try_from(pause.as_millis()).unwrap_or(u64::MAX);
                let at = now_ms().saturating_add(pause);
                self.access.db().mail_failed(to, root, mail, attempts, at)
            }
        }
    }

    /// One attempt to deliver `entry`'s message.
    async fn deliver(&self, entry: OutboxEntry) -> Delivery {
        let (to, root) = (entry.to, entry.root);
        let share = match self.access.db().share(root) {
            Ok(Some(share)) => share,
            Ok(None) => return Delivery::Dropped,
            Err(_) => return Delivery::Failed,
        };
        let (message, expected) = match entry.mail {
            Mail::Offer => {
                // No longer a recipient, or the share is withdrawn: not even the
                // title may reach them.
                if !share.recipients.contains(&to) || !share.is_active(now_ms()) {
                    return Delivery::Dropped;
                }
                let offer = ControlMsg::ShareOffer {
                    root,
                    title: share.title,
                    file_count: share.file_count,
                    total_size: share.total_size,
                    preview: share.preview,
                    expires_at: share.expires_at,
                };
                (offer, ControlMsg::OfferReceived)
            }
            Mail::Revoke => (ControlMsg::Revoke { root }, ControlMsg::Ack),
        };
        let answer =
            tokio::time::timeout(self.timing.offer_timeout, self.request(to, &message)).await;
        match answer {
            Ok(Ok(reply)) if reply == expected => Delivery::Delivered,
            // Any other answer means it was read and not taken.
            Ok(Ok(_) | Err(ControlError::Refused)) => Delivery::Dropped,
            Ok(Err(_)) | Err(_) => Delivery::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::TIMING;

    #[tokio::test]
    async fn a_crashed_task_is_restarted_until_it_ends_normally() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let (events, mut listener) = broadcast::channel(8);
        let runs = Arc::new(AtomicU32::new(0));
        let counted = Arc::clone(&runs);

        supervise(
            events,
            "a test task",
            Duration::from_millis(10),
            move || {
                let run = counted.fetch_add(1, Ordering::SeqCst);
                async move {
                    assert!(run > 0, "first run crashes");
                }
            },
        )
        .await;

        assert_eq!(runs.load(Ordering::SeqCst), 2);
        assert!(matches!(
            listener.try_recv(),
            Ok(Event::Warning {
                context: "a test task",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn a_delivery_that_panics_still_frees_its_offer() {
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let key = (PeerId::from([1; 32]), ContentHash::from([2; 32]));
        in_flight.lock().unwrap().insert(key);
        let on_its_way = OnItsWay {
            key,
            in_flight: Arc::clone(&in_flight),
            postman: Arc::new(Notify::new()),
        };

        let task = tokio::spawn(async move {
            let _on_its_way = on_its_way;
            panic!("a delivery crashed");
        });

        assert!(task.await.unwrap_err().is_panic());
        assert!(in_flight.lock().unwrap().is_empty());
    }

    #[test]
    fn pauses_double_from_30_seconds_up_to_10_minutes() {
        let secs = |attempts| retry_delay(attempts, &TIMING).as_secs();
        assert_eq!(
            [1, 2, 3, 4, 5, 6, 7, 100].map(secs),
            [30, 60, 120, 240, 480, 600, 600, 600]
        );
    }
}
