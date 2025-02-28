// Copyright 2024 Aleo Network Foundation
// This file is part of the snarkOS library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:

// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use snarkvm::{
    ledger::narwhal::{BatchCertificate, Subdag},
    prelude::{Address, Field, Network, cfg_iter},
};

use indexmap::{IndexMap, IndexSet};
use parking_lot::RwLock;
use rayon::prelude::*;
use std::sync::Arc;

/// Tracker for the participation metrics of validators.
#[derive(Clone, Debug)]
pub struct Telemetry<N: Network> {
    /// The certificates seen for each round
    /// A mapping of `round` to set of certificate IDs.
    tracked_certificates: Arc<RwLock<IndexMap<u64, IndexSet<Field<N>>>>>,

    /// The total number of signatures seen for a validator, including for their own certificates.
    /// A mapping of `address` to a mapping of `round` to `count`.
    validator_signatures: Arc<RwLock<IndexMap<Address<N>, IndexMap<u64, u32>>>>,

    /// The total number of certificates seen for a validator.
    /// A mapping of `address` to a list of rounds.
    validator_certificates: Arc<RwLock<IndexMap<Address<N>, IndexSet<u64>>>>,
    // TODO (raychu86): Consider adding dedicated metric variable for quick lookup.
    //  For instance making `participation_scores` a getter, and all other functions update
    //  final metric numbers.
}

impl<N: Network> Default for Telemetry<N> {
    /// Initializes a new instance of telemetry.
    fn default() -> Self {
        Self::new()
    }
}

impl<N: Network> Telemetry<N> {
    /// Initializes a new instance of telemetry.
    pub fn new() -> Self {
        Self {
            tracked_certificates: Default::default(),
            validator_signatures: Default::default(),
            validator_certificates: Default::default(),
        }
    }

    // TODO: Determine if this should be pending signatures/certificates, or only certificates included in a fully formed subdag.
    pub fn insert_subdag(&self, subdag: &Subdag<N>) {
        // Insert the subdag certificates
        cfg_iter!(subdag).for_each(|(_round, certificates)| {
            cfg_iter!(certificates).for_each(|certificate| {
                // TODO (raychu86): Can be greatly optimized by doing a one-shot update instead of individual certificates.
                self.insert_certificate(certificate);
            })
        });
    }

    /// Insert a certificate to the telemetry tracker.
    pub fn insert_certificate(&self, certificate: &BatchCertificate<N>) {
        // Acquire the lock for `tracked_certificates`.
        let mut tracked_certificates = self.tracked_certificates.write();

        // Retrieve the certificate round, author, and ID.
        let certificate_round = certificate.round();
        let certificate_author = certificate.author();
        let certificate_id = certificate.id();

        // If the certificate already exists in the tracker, then return early.
        if tracked_certificates.get(&certificate_round).map_or(false, |certs| certs.contains(&certificate_id)) {
            return;
        }

        // Insert the certificate ID.
        tracked_certificates
            .entry(certificate_round)
            .and_modify(|certs| {
                certs.insert(certificate_id);
            })
            .or_insert_with(|| IndexSet::from([certificate_id]));

        // Acquire the lock for `validator_signatures`
        let mut validator_signatures = self.validator_signatures.write();

        // Insert the certificate author.
        validator_signatures
            .entry(certificate_author)
            .and_modify(|counts| {
                counts.entry(certificate_round).and_modify(|count| *count += 1).or_insert(1);
            })
            .or_insert_with(|| IndexMap::from([(certificate_round, 1)]));

        // Insert the certificate signatures
        for signature in certificate.signatures() {
            validator_signatures
                .entry(signature.to_address())
                .and_modify(|counts| {
                    counts.entry(certificate_round).and_modify(|count| *count += 1).or_insert(1);
                })
                .or_insert_with(|| IndexMap::from([(certificate_round, 1)]));
        }

        // Acquire the lock for `validator_certificates`.
        let mut validator_certificates = self.validator_certificates.write();

        // Insert the certificate
        validator_certificates
            .entry(certificate_author)
            .and_modify(|cert_rounds| {
                cert_rounds.insert(certificate_round);
            })
            .or_insert_with(|| IndexSet::from([certificate_round]));
    }

    /// Remove the certificates from the telemetry tracker that are no longer relevant based on gc.
    pub fn garbage_collect_certificates(&self, gc_round: u64) {
        // Aquire the locks.
        let mut tracked_certificates = self.tracked_certificates.write();
        let mut validator_signatures = self.validator_signatures.write();
        let mut validator_certificates = self.validator_certificates.write();

        // Remove certificates that are not longer relevant
        tracked_certificates.retain(|&round, _| round > gc_round);

        // Remove signatures that are no longer relevant.
        validator_signatures.retain(|_, rounds| {
            rounds.retain(|&round, _| round > gc_round);
            // Remove the address if there are no more tracked signatures.
            !rounds.is_empty()
        });

        // Remove certificates that are no longer relevant.
        validator_certificates.retain(|_, rounds| {
            rounds.retain(|&round| round > gc_round);
            // Remove the address if there are no more tracked certificates.
            !rounds.is_empty()
        });
    }

    // TODO (raychu86): Add functions to determine which validator is not participating based on committee lookbacks.
    // pub fn participation_scores(&self) -> IndexMap<Address<N>, u8> {
    //
    // }

    // TODO (raychu86): Determine how to perform calculations for incoming and outgoing validators.
}
