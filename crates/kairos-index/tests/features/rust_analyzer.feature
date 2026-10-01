# COLLIERY-T-1858: the index uses a pinned standalone rust-analyzer release,
# not the one of the toolchain. The scenarios use the binary in
# ~/.cache/kairos-index/bin/ (`angreal dev fetch-rust-analyzer`) and never
# download it. The scenario "The SCIP run still builds nothing" is now in
# std_source.feature (COLLIERY-T-1860).
Feature: A pinned rust-analyzer

  Scenario: Test crates that share a module are indexed
    Given the polyglot fixture, where 3 test crates share tests/common/mod.rs
    When I build the index with the pinned rust-analyzer
    Then no target is left out
    And each call from the 3 test crates to the shared module is a "certain" edge from SCIP

  Scenario: The pinned binary is checked
    Given a rust-analyzer binary whose sha256 is not the pinned value
    When I build the index
    Then the build stops, and the error names the binary and the expected checksum
