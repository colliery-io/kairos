# COLLIERY-T-1860: the index uses the latest rust-analyzer with a pinned std
# source from Rust 1.94 or later. rust-analyzer finds the std macros only
# through the prelude, as std does since Rust 1.94 (COLLIERY-T-1859). The
# scenarios use the archive in ~/.cache/kairos-index/rust-src/
# (`angreal dev fetch-rust-analyzer`) and never download it.
Feature: The latest rust-analyzer with a pinned std source

  Scenario: Calls inside std macros resolve
    Given the polyglot fixture, where a Rust function calls a fixture function inside assert_eq!, format!, vec! and println!
    When I build the index with the pinned rust-analyzer and the pinned std source
    Then each of the 4 calls is a "certain" edge from SCIP

  Scenario: The std source of the repository's toolchain is not used
    Given a fixture whose rust-toolchain.toml pins Rust 1.93
    When I build the index
    Then rust-analyzer reads the pinned std source, and the log shows its path

  Scenario: The pinned std source is checked
    Given a std source archive whose sha256 is not the pinned value
    When I build the index
    Then the build stops, and the error names the archive and the expected checksum

  # This replaces the scenario of the same name of COLLIERY-T-1858.
  Scenario: The SCIP run still builds nothing
    Given the polyglot fixture with no target folder
    When I build the index
    Then the fixture still has no target folder
    And the log shows the build-script command "true" and no proc-macro server
