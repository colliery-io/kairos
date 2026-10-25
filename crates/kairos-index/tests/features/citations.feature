# KAIROS-T-0703: the index records each ADR short code in a comment, a doc
# string or the name of a test, with its file, its line and its symbol. So
# "what code depends on this ADR?" has an answer that is not a text search.
Feature: The ADR citations of the code

  Scenario: Each language gives the citations of its comments, its doc strings and its test names
    Given the polyglot fixture with ADR citations in each language
    When I build the structure
    Then the places that cite "KAIROS-A-0065" are:
      | place                        | context | symbol                         |
      | go/greet/retry.go:3          | doc     | Retry                          |
      | infra/retry.tf:1             | comment |                                |
      | migrations/002_keys/up.sql:1 | comment |                                |
      | python/polyglot/retry.py:2   | doc     |                                |
      | python/polyglot/retry.py:6   | doc     | retry                          |
      | src/decisions.rs:1           | doc     |                                |
      | src/decisions.rs:3           | doc     | retry_write                    |
      | src/decisions.rs:5           | comment | retry_write                    |
      | src/decisions.rs:14          | test    | retry_is_safe_kairos_a_0065    |
      | web/src/retry.ts:2           | doc     | retryWrite                     |
      | web/src/retry.ts:5           | comment | retryWrite                     |
    And the code cites "KAIROS-A-0065" in 11 places and 6 files
    And the code cites "COLLIERY-A-0023" in 1 place and 1 file
    But no place cites "KAIROS-A-0999", because a code in a string is not a citation

  Scenario: An update reads the citations of a changed file again
    Given the polyglot fixture with ADR citations in each language
    When I build the structure
    And I add a comment that cites "KAIROS-A-0070" at the end of "src/decisions.rs" and update the index
    Then the places that cite "KAIROS-A-0070" are:
      | place               | context | symbol |
      | src/decisions.rs:18 | comment |        |
    And the code cites "KAIROS-A-0065" in 11 places and 6 files
