"""A fixture: code that a test reads as data. It is not part of the program."""


def handle(event):
    return {"status": 200, "body": event}
