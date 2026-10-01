# COLLIERY-T-1848: the structure of the index, from the polyglot fixture.
Feature: Extract the structure of a repository

  Scenario: Each language gives its symbols
    Given the polyglot fixture repository
    When I build the structure
    Then the index has the symbols listed in the fixture's expected-symbols file
    And each symbol has its file, its span, its kind and its language

  Scenario: Vendored, generated and fixture files are not indexed
    Given the polyglot fixture repository
    When I build the structure
    Then the vendored file, the generated file and the fixture file have no symbols
    And the decision for each of them is recorded with its rule

  Scenario: Test code is in the structure but is not summarized
    Given the polyglot fixture repository
    When I build the structure
    Then the symbols of the test file are in the index
    And they are marked as test code

  Scenario: A repository rule changes a decision
    Given the polyglot fixture repository with a rule that excludes "scripts/"
    When I build the structure
    Then no file under "scripts/" has symbols

  Scenario: Building twice gives the same index
    Given the polyglot fixture repository
    When I build the structure two times
    Then the 2 indexes have the same symbols and the same hashes
