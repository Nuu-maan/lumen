import json
import sys
import time

import uno
from com.sun.star.beans import PropertyValue
from com.sun.star.connection import NoConnectException

FILTERS = [
    ("com.sun.star.sheet.SpreadsheetDocument", "calc_pdf_Export"),
    ("com.sun.star.presentation.PresentationDocument", "impress_pdf_Export"),
    ("com.sun.star.drawing.DrawingDocument", "draw_pdf_Export"),
    ("com.sun.star.text.TextDocument", "writer_pdf_Export"),
]


def prop(name, value):
    p = PropertyValue()
    p.Name = name
    p.Value = value
    return p


def reply(**payload):
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def connect(pipe):
    local = uno.getComponentContext()
    resolver = local.ServiceManager.createInstanceWithContext("com.sun.star.bridge.UnoUrlResolver", local)
    deadline = time.time() + 90
    while True:
        try:
            return resolver.resolve("uno:pipe,name=%s;urp;StarOffice.ComponentContext" % pipe)
        except NoConnectException:
            if time.time() > deadline:
                raise
            time.sleep(0.05)


def convert(desktop, request):
    load = [
        prop("Hidden", True),
        prop("ReadOnly", True),
        prop("MacroExecutionMode", 0),
        prop("UpdateDocMode", 0),
        prop("RepairPackage", False),
    ]
    if request.get("password"):
        load.append(prop("Password", request["password"]))
    doc = desktop.loadComponentFromURL(uno.systemPathToFileUrl(request["in"]), "_blank", 0, tuple(load))
    if doc is None:
        raise RuntimeError("load failed")
    try:
        if doc.supportsService("com.sun.star.text.TextDocument"):
            doc.setPropertyValue("RedlineDisplayType", 0)
        export = next((f for service, f in FILTERS if doc.supportsService(service)), "writer_pdf_Export")
        args = [prop("FilterName", export)]
        if request.get("pages") and export == "calc_pdf_Export":
            # Big sheets print to thousands of pages; only the requested range is exported.
            data = uno.Any("[]com.sun.star.beans.PropertyValue", (prop("PageRange", request["pages"]),))
            args.append(prop("FilterData", data))
        uno.invoke(doc, "storeToURL", (uno.systemPathToFileUrl(request["out"]), tuple(args)))
    finally:
        doc.close(True)


def main():
    ctx = connect(sys.argv[1])
    desktop = ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", ctx)
    reply(ready=True)
    for line in sys.stdin:
        try:
            convert(desktop, json.loads(line))
            reply(ok=True)
        except Exception as e:
            reply(error="%s: %s" % (type(e).__name__, getattr(e, "Message", "") or e))
    try:
        desktop.terminate()
    except Exception:
        pass


main()
