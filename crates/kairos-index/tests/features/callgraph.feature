# COLLIERY-T-1849: the call graph of the index, from the polyglot fixture.
# The Rust edges come from `rust-analyzer scip`, so these scenarios need the
# pinned rust-analyzer and the pinned std source
# (`angreal dev fetch-rust-analyzer`).
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

  # "The SCIP run builds nothing" is now in rust_analyzer.feature
  # (COLLIERY-T-1858).

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

  # The pinned rust-analyzer indexes test crates that share a module
  # (rust_analyzer.feature, COLLIERY-T-1858). rust-analyzer 1.93.0 panicked
  # on them, and the index left them out (COLLIERY-T-1849).

  # A defect found on Kairos: a SCIP symbol names the package and the path,
  # not the crate, so the root functions of 2 test crates share a symbol.
  Scenario: A function at the root of 2 test crates resolves in its own crate
    Given the polyglot fixture, where 2 Rust test crates each define a function "setup"
    When I build the index
    Then the call of setup in each test crate goes to the setup of that crate
