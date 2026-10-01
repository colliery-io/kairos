# COLLIERY-T-1849: the call graph of the index, from the polyglot fixture.
# The Rust edges come from `rust-analyzer scip`, so these scenarios need the
# rustup components rust-analyzer and rust-src.
Feature: The call graph

  Scenario: A Rust method call resolves to its real target
    Given the polyglot fixture, where 2 Rust types each have a method named "push"
    And a function that calls push on a value of the second type
    When I build the index
    Then the edge from that function goes to the second type's push
    And the edge is "certain"

  Scenario: A Rust trait method call resolves through the trait
    Given the polyglot fixture, where a function calls a trait method on a generic value
    When I build the index
    Then the edge goes to the trait method
    And the edge is "certain"

  Scenario: A Rust call to a trait method with no body resolves to its declaration
    Given the polyglot fixture, where a function calls a trait method with no body on a generic value
    When I build the index
    Then the edge goes to the trait method
    And the edge is "certain"

  Scenario: The SCIP run builds nothing
    Given the polyglot fixture with no target folder
    When I build the index
    Then the fixture still has no target folder
    And no temporary build folder is left after the run

  Scenario: An ambiguous Python call lists its candidates
    Given the polyglot fixture, where 2 Python modules each define "load"
    And a function that calls load with no import that decides it
    When I build the index
    Then the edge is "possible"
    And it names the 2 candidates

  Scenario: A call into the standard library is external
    Given a Go function that calls fmt.Println
    When I build the index
    Then the edge is "external"

  Scenario: The edges match the fixture's expected edges
    Given the polyglot fixture repository
    When I build the index
    Then each edge in the fixture's expected-edges file is in the index with its class
    And the index has no other edge

  # A defect found on Kairos: rust-analyzer 1.93.0 `scip` panics on a file
  # in 2 crates ("Invariant violation: file emitted multiple times").
  Scenario: A module that 2 test crates share does not stop the SCIP run
    Given the polyglot fixture, where 2 Rust test crates share the module "tests/common/mod.rs"
    When I build the index
    Then the SCIP run leaves out the test crate "tests/second.rs"
    And each call from "tests/second.rs" has a name class

  # A defect found on Kairos: a SCIP symbol names the package and the path,
  # not the crate, so the root functions of 2 test crates share a symbol.
  Scenario: A function at the root of 2 test crates resolves in its own crate
    Given the polyglot fixture, where 2 Rust test crates each define a function "setup"
    When I build the index
    Then the call of setup in each test crate goes to the setup of that crate
