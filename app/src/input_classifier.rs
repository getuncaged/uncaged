//! Uncaged: the input classifier is built on first use, not at startup.
//!
//! `app::initialize_app` registers this as a singleton, and `add_singleton_model` constructs
//! eagerly. The constructor used to load the ONNX model right there: `nld_classifier_v3` is
//! appended to the feature list by every bundle script, so a shipped build prost-decoded a
//! 17.5 MB BERT-tiny graph and built a 30,522-entry tokenizer vocabulary on the main thread,
//! before the first window existed.
//!
//! It did that unconditionally -- no cfg, no feature check, no setting -- for a classifier
//! that is **off by default** (`settings/ai.rs`, and `settings/initializer.rs` force-clears
//! pre-existing `true` values). The cost was paid by every user on every launch, and by the
//! resident set for the whole session, whether or not a single classification ever ran.
//!
//! Now the model is built the first time something asks for a classifier, and cached. When
//! autodetection is off -- the normal case -- that is never, and neither the decode nor the
//! vocabulary is ever allocated.

use std::sync::Arc;

use input_classifier::{HeuristicClassifier, InputClassifier};
#[cfg(any(
    feature = "nld_classifier_v1",
    feature = "nld_classifier_v2",
    feature = "nld_classifier_v3"
))]
use input_classifier::{OnnxClassifier, OnnxModel};
use once_cell::sync::OnceCell;
use warpui::{Entity, ModelContext, SingletonEntity};

pub struct InputClassifierModel {
    /// Built on first request. `OnceCell` rather than a plain `Option` so `classifier()` can
    /// stay `&self` -- callers reach this through `as_ref(ctx)` and have no `&mut`.
    classifier: OnceCell<Arc<dyn InputClassifier>>,
}

impl InputClassifierModel {
    pub fn new(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            classifier: OnceCell::new(),
        }
    }

    /// The classifier, loading the ONNX model on the first call if one is compiled in.
    ///
    /// Callers are already async and already treat classification as fallible and abortable,
    /// so a slower first call is safe. Every later call is a clone of an `Arc`.
    pub fn classifier(&self) -> Arc<dyn InputClassifier> {
        self.classifier.get_or_init(Self::build_classifier).clone()
    }

    fn build_classifier() -> Arc<dyn InputClassifier> {
        #[cfg(feature = "nld_classifier_v1")]
        {
            match OnnxClassifier::new(OnnxModel::BertTinyV1) {
                Ok(classifier) => {
                    log::info!("Loaded onnx classifier bert_tiny_v1.onnx");
                    return Arc::new(classifier);
                }
                Err(e) => log::warn!("Failed to load onnx classifier bert_tiny_v1.onnx: {e:#}"),
            }
        }

        #[cfg(feature = "nld_classifier_v2")]
        {
            match OnnxClassifier::new(OnnxModel::BertTinyV2) {
                Ok(classifier) => {
                    log::info!("Loaded onnx classifier bert_tiny_v2.onnx");
                    return Arc::new(classifier);
                }
                Err(e) => log::warn!("Failed to load onnx classifier bert_tiny_v2.onnx: {e:#}"),
            }
        }

        #[cfg(feature = "nld_classifier_v3")]
        {
            match OnnxClassifier::new(OnnxModel::BertTinyV3) {
                Ok(classifier) => {
                    log::info!("Loaded onnx classifier bert_tiny_v3.onnx");
                    return Arc::new(classifier);
                }
                Err(e) => log::warn!("Failed to load onnx classifier bert_tiny_v3.onnx: {e:#}"),
            }
        }

        Arc::new(HeuristicClassifier)
    }
}

impl Entity for InputClassifierModel {
    type Event = ();
}

impl SingletonEntity for InputClassifierModel {}
