"""Что Qt-программа видит на самом деле: стиль, шрифты, тема значков, палитра."""
import os, sys
from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QIcon, QFontDatabase, QGuiApplication

app = QApplication(sys.argv)
pt = os.environ.get("QT_QPA_PLATFORMTHEME", "")
print("QT_QPA_PLATFORMTHEME=%r QT_STYLE_OVERRIDE=%r" % (pt, os.environ.get("QT_STYLE_OVERRIDE", "")))
st = app.style()
base = st.baseStyle() if hasattr(st, "baseStyle") else None
print("style      =", st.name(), "| base:", base.name() if base else "-")
print("font       =", app.font().family(), app.font().pointSize())
print("fixed      =", QFontDatabase.systemFont(QFontDatabase.FixedFont).family(),
      QFontDatabase.systemFont(QFontDatabase.FixedFont).pointSize())
print("iconTheme  =", QIcon.themeName(), "| fallback:", QIcon.fallbackThemeName())
print("window bg  =", app.palette().window().color().name())
