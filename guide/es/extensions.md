[English](../en/extensions.md) | [Русский](../ru/extensions.md) | [Deutsch](../de/extensions.md) | [简体中文](../zh-CN/extensions.md) | [Français](../fr/extensions.md) | Español | [Українська](../uk/extensions.md)

# Extensiones

En esta pestaña viven los paquetes `.rxn`, el formato propio de complementos
de Rexon: temas, colecciones de fondos de pantalla, tipos de letra,
herramientas para el escritorio y módulos. Aquí también puedes soltar un tema
o unos cursores de Windows: Rexon los convertirá para Linux.

Las extensiones de GNOME Shell y Cinnamon no se instalan aquí, sino en la
pestaña [Apariencia](appearance.md), en las secciones «Extensiones de GNOME» y
«Extensiones de Cinnamon».

![La pestaña «Extensiones», todavía sin nada instalado](../images/extensions-overview.png)

## Qué hay en la pestaña

- **Selectores de vista** «Cuadrícula» (⊞) y «Lista» (☰): recuadros con
  imagen o filas compactas.
- **＋ Instalar desde archivo**: elegir un archivo del disco.
- **Búsqueda**: por nombre, autor y etiquetas.
- **Filtros por tipo**: «Todas» y un botón para cada tipo que ya tengas
  instalado (por ejemplo, solo temas GTK o solo herramientas).
- **Línea de recuento**: «Instaladas» y «Activas».
- **Mis extensiones**: los paquetes instalados. Cada uno tiene:
  - **▶️ Activar** / **⏸️ Desactivar**: en herramientas, módulos y
    complementos de escritorio (scripts y efectos de KWin, extensiones de
    GNOME, applets y desklets de Cinnamon, acciones de Nemo). Después de
    instalarlos están desactivados: los activas tú;
  - **Aplicar**: en las colecciones de fondos de pantalla;
  - **🗑️ Desinstalar**.

  Un clic en el recuadro abre la ventana «Extensión» con la descripción y el
  botón **📁 Abrir carpeta**.
- **⬇️ Instalar**: tres formas:
  - **Desde archivo**: abrir un `.rxn`, un `.themepack` o un tema de Windows;
  - **Workshop de Steam — próximamente**: todavía no funciona, llegará más
    adelante;
  - **Crear la tuya**: una guía rápida para crear tu propia extensión.

## Cómo se hace

### Instalar una extensión

1. Pulsa **＋ Instalar desde archivo** (o **Desde archivo** en la sección
   «⬇️ Instalar»).
2. Elige el archivo `.rxn`.
3. Espera a que aparezca la línea «✅ Instalado: …» en la parte inferior de la
   ventana.

O, más fácil: arrastra el archivo directamente a la ventana de Rexon.

### Instalar un tema o cursores de Windows

Arrástralos a la ventana o ábrelos con **Desde archivo**:

- temas: `.themepack`, `.deskthemepack`, `.theme`, `.msstyles`;
- cursores: `.cur`, `.ani`, `.inf`, `.curxptheme`;
- conjuntos de iconos: `.7tsp`.

Rexon los convertirá en un paquete `.rxn` y lo instalará: el paquete aparecerá
en «Mis extensiones», y el tema en sí, en [Apariencia](appearance.md), en la
sección de su tipo. Se aplica desde allí.

### Buscar una extensión

Empieza a escribir el nombre, el autor o una etiqueta en la barra de
búsqueda. Para dejar solo un tipo de paquetes, pulsa el botón del tipo que
quieras debajo de la barra de búsqueda; «Todas» quita el filtro.

### Activar o desactivar

Pulsa **▶️ Activar** en el recuadro: abajo aparecerá «▶️ Activado: …». Para
desactivar, **⏸️ Desactivar**. Los temas y los tipos de letra no tienen este
botón: simplemente quedan instalados y se aplican en
[Apariencia](appearance.md).

### Desinstalar

1. Pulsa **🗑️ Desinstalar** en el recuadro o en la ventana «Extensión».
2. Confirma en la ventana «¿Eliminar la extensión?». Los archivos de la
   extensión se eliminan de forma permanente.

### Crear tu propia extensión `.rxn`

Pulsa **Crear la tuya**. Rexon mostrará una guía rápida: un `.rxn` es un
archivo ZIP normal con un archivo `manifest.json` (nombre, versión, autor,
tipo), una imagen para el catálogo y los archivos de la propia extensión. La
guía incluye un ejemplo de `manifest.json` y la lista de tipos.

![La guía «Crea tu propia extensión .rxn»](../images/extensions-create.png)

## Si algo sale mal

- **«⚠️ Ya hay una instalación en curso».** Espera a que termine la
  instalación o desinstalación anterior y vuelve a intentarlo.
- **«⚠️ Error: …».** El archivo está dañado o no es una extensión de Rexon.
  El texto tras los dos puntos indica el motivo. Si la línea «Instalado»
  incluye salvedades (por ejemplo, que no se ha encontrado la aplicación
  necesaria para convertir un tema de Windows), la extensión se ha instalado,
  pero no por completo.
- **«⚠️ Error al activar o desactivar: …».** No se ha podido activar o
  desactivar la extensión. A menudo ayuda cerrar la sesión y volver a
  iniciarla, sobre todo con las extensiones de GNOME y Cinnamon.
- **Después de activar una extensión, el escritorio se comporta de forma
  extraña.** Desactívala con el botón **⏸️ Desactivar** o desinstálala. Si el
  escritorio se ha roto del todo, inicia sesión en otro escritorio y pulsa
  **🔧 Comprobar / Reparar** en la tarjeta del escritorio con problemas, en la
  pestaña [Escritorios](desktops.md).
- **Cuidado con las herramientas y los módulos.** Los paquetes de estos dos
  tipos ejecutan sus propios scripts de instalación y desinstalación (en tu
  nombre, sin permisos de administrador). Instálalos solo si confías en
  quien los ha hecho.

## Véase también

- [Escritorios](desktops.md)
- [Apariencia](appearance.md): dónde se aplican los temas y dónde están las
  extensiones de GNOME y Cinnamon
- [Wallpaper Engine](wallpaper-engine.md)
- [Steam](steam.md)
- [Ajustes](settings.md)
- [Índice de la guía](../README.md)
