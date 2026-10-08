Feature: SQL files are indexed

  # KAIROS-T-0350. The migrations of a repository were `source` files with no
  # language: no symbol, no summary, no place in a search. A SQL file now
  # gives the things its statements define, and they get summaries as the
  # other symbols do.

  Scenario: A migration gives its table and its index as symbols
    Given an index of the polyglot fixture
    Then migrations/001_payments/up.sql has the language sql and no parse error
    And it has the table payments on lines 2 to 6 and the index payments_by_day on line 8
    And migrations/001_payments/down.sql has the language sql and no symbol

  Scenario: SQL symbols get summaries and a search finds a table
    Given the polyglot fixture and the fake summarizer
    When I summarize the index
    Then the table payments has a summary, from its statement
    And the module migrations/001_payments has a summary
    When I search the code for "the payments table"
    Then the table payments is in the first 3 results
