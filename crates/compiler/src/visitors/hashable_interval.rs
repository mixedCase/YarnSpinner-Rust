use crate::parser::generated::yarnspinnerparser::YarnSpinnerParserContext;
use antlr4rust::interval_set::Interval;
use antlr4rust::parser_rule_context::ParserRuleContext;
use antlr4rust::token::Token;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};
use yarnspinner_core::prelude::*;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) struct HashableInterval(Interval);

impl From<Interval> for HashableInterval {
    fn from(interval: Interval) -> Self {
        Self(interval)
    }
}

impl Ord for HashableInterval {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.a.cmp(&other.0.a).then(self.0.b.cmp(&other.0.b))
    }
}

impl PartialOrd for HashableInterval {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for HashableInterval {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.a.hash(state);
        self.0.b.hash(state);
    }
}

impl Deref for HashableInterval {
    type Target = Interval;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HashableInterval {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(crate) trait GetHashableInterval<'input>: ParserRuleContext<'input> {
    fn get_hashable_interval(&self) -> HashableInterval {
        // FIXME: The get_source_interval function only seems to return INVALID values.
        //        I'm not sure if this is a bug in antlr4rust of an issue with the types in this function.
        let interval = Interval {
            a: self.start().get_token_index() as i32,
            b: self.stop().get_token_index() as i32,
        };
        HashableInterval(interval)
    }
}

impl<'input, T: ParserRuleContext<'input> + ?Sized> GetHashableInterval<'input> for T {}
