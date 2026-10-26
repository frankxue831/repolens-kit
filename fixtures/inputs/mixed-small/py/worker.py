"""Worker module for mixed-small fixture."""


class Worker:
    """A simple worker class."""

    def __init__(self, name: str) -> None:
        self.name = name

    def run(self) -> str:
        return process(self.name)


def process(value: str) -> str:
    """Process a value and return a result string."""
    return f"processed: {value}"
