"""What `ValidationError.to_problem` needs to build an RFC 9457 problem details object."""

__all__ = ["check_status", "status_title"]

_TITLES = {
    400: "Bad Request",
    401: "Unauthorized",
    402: "Payment Required",
    403: "Forbidden",
    404: "Not Found",
    405: "Method Not Allowed",
    406: "Not Acceptable",
    407: "Proxy Authentication Required",
    408: "Request Timeout",
    409: "Conflict",
    410: "Gone",
    411: "Length Required",
    412: "Precondition Failed",
    413: "Content Too Large",
    414: "URI Too Long",
    415: "Unsupported Media Type",
    416: "Range Not Satisfiable",
    417: "Expectation Failed",
    421: "Misdirected Request",
    422: "Unprocessable Content",
    423: "Locked",
    424: "Failed Dependency",
    425: "Too Early",
    426: "Upgrade Required",
    428: "Precondition Required",
    429: "Too Many Requests",
    431: "Request Header Fields Too Large",
    451: "Unavailable For Legal Reasons",
    500: "Internal Server Error",
    501: "Not Implemented",
    502: "Bad Gateway",
    503: "Service Unavailable",
    504: "Gateway Timeout",
    505: "HTTP Version Not Supported",
    506: "Variant Also Negotiates",
    507: "Insufficient Storage",
    508: "Loop Detected",
    510: "Not Extended",
    511: "Network Authentication Required",
}
"""The phrases of RFC 9110 and the status code registry; `http.HTTPStatus` spells some
differently by Python version."""


def check_status(status: object) -> int:
    """The HTTP error status as a plain `int` (an `http.HTTPStatus` member is converted)."""
    if isinstance(status, bool) or not isinstance(status, int) or not 400 <= status <= 599:
        raise ValueError(
            f"status must be an HTTP error status from 400 to 599, not {status!r}; "
            "pass the status of the response the problem is sent with"
        )
    return int(status)


def status_title(status: int) -> str:
    """The recommended phrase of an HTTP error status, for the `title` of a problem."""
    title = _TITLES.get(status)
    if title is None:
        raise ValueError(
            f"oxiform has no standard title for status {status}; pass title= to to_problem()"
        )
    return title
