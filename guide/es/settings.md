[English](../en/settings.md) | [Русский](../ru/settings.md) | [Deutsch](../de/settings.md) | [简体中文](../zh-CN/settings.md) | [Français](../fr/settings.md) | Español | [Українська](../uk/settings.md)

# Ajustes

Aquí se configura el propio Rexon (idioma, aspecto de la ventana, icono en la
bandeja) y algunas cosas comunes para todo el ordenador: qué gestor de
archivos abre las carpetas, qué aplicaciones abren los archivos de forma
predeterminada, fondos de pantalla a partir de una imagen o un vídeo, sonidos
de fondo y sonidos de eventos. Las secciones van de arriba abajo en una sola
columna; algunas solo aparecen donde tienen sentido (por ejemplo, la
protección del panel, solo en escritorios cuyo panel Rexon sabe guardar).

![La pestaña «Ajustes»: idioma, aspecto de la interfaz, recorrido](../images/settings-top.png)

## Qué hay en la pestaña

| Sección | Para qué sirve |
| --- | --- |
| 🌐 **Idioma** | El idioma de las ventanas, menús y mensajes de Rexon |
| 🎨 **Aspecto de la interfaz** | Los colores de la ventana del propio Rexon |
| **Recorrido en el primer inicio** | Consejos breves sobre la ventana |
| 🔔 **Icono en la bandeja del sistema** | El icono junto al reloj y el inicio automático |
| 📁 **Gestor de archivos** | Qué aplicación abre las carpetas |
| 🔧 **Restauración rápida de aplicaciones** | Recuperar las aplicaciones propias del escritorio actual |
| 🧩 **Integridad del escritorio** | Comprobar que el escritorio tiene instalado todo lo que necesita |
| 🎬 **Fondo de pantalla (imagen / vídeo / YouTube / web)** | Fondos de pantalla desde un archivo, un vídeo de YouTube o Vimeo, una página web |
| 📚 **Biblioteca de fondos de pantalla** | Paquetes de fondos de pantalla de Rexon (.rxn) |
| 🪟 **Importar temas de Windows** | Traer la apariencia de tu antiguo ordenador con Windows |
| 🔒 **Protección de aplicaciones predeterminadas** | Impedir que un escritorio nuevo sustituya tus aplicaciones |
| 🛡️ **Protección del panel** | Copias de los paneles, los applets y los atajos de teclado de cualquier escritorio |
| 🪟 **Apariencia del escritorio** | Solo si hay instalado un fork de la apariencia de Plasma (AeroShell, etc.) |

Las animaciones, los tipos de letra, los botones de la barra de título, la
respuesta al iniciar aplicaciones, los iconos de las aplicaciones, el
desenfoque del fondo de pantalla, los sonidos de fondo y los sonidos de
eventos están en la pestaña «Apariencia», igual que en las Preferencias del
sistema de KDE.

Si ya has introducido la contraseña, en la parte superior de la pestaña se ve
la franja «🔓 Rexon puede modificar el sistema hasta que cierres la ventana».
El botón **«Prohibir ahora»** retira este permiso: la próxima vez, Rexon
volverá a pedirte la contraseña.

## Cómo cambiar el idioma y el aspecto

1. En la sección **🌐 Idioma**, elige el idioma en la lista. La primera
   opción es «Como en el sistema».
2. El idioma cambia al reiniciar Rexon: pulsa **«Reiniciar Rexon»**.
3. En la sección **🎨 Aspecto de la interfaz**, elige el aspecto de la
   ventana: «Oscuro (Steam)», el predeterminado, «Claro», «Oscuro (Material)»,
   «Synthwave / Neon Space Retro» o «Sistema (seguir el tema del sistema)».
   El aspecto cambia al instante.

El aspecto solo afecta a los colores de la ventana de Rexon, no al tema de
todo el escritorio. El tema del escritorio se cambia en la pestaña
«Apariencia».

## Recorrido en el primer inicio

La primera vez que se inicia, Rexon pregunta «¿Mostrar un recorrido breve?».
El recorrido dura alrededor de un minuto y muestra dónde se cambian los temas,
los fondos de pantalla y la apariencia.

![La pregunta «¿Mostrar un recorrido breve?»](../images/settings-tour.png)

- La casilla **«Mostrar el tutorial en el primer inicio»** activa y desactiva
  esta pregunta.
- El botón **«Repetir el recorrido»** inicia el recorrido en cualquier
  momento.

## Icono en la bandeja e inicio automático

![El icono en la bandeja del sistema y el gestor de archivos](../images/settings-tray.png)

Rexon mantiene un icono junto al reloj: desde allí tienes a mano los fondos
de pantalla, las secciones y la salida, sin abrir la ventana. Mientras el
icono está ahí, la cruz de la ventana esconde Rexon en la bandeja en lugar de
cerrarlo.

- **«Iniciar al iniciar sesión»**: Rexon se abrirá en la bandeja al iniciar
  sesión, sin mostrar la ventana.
- **«Descargar automáticamente los iconos y cursores que necesite el
  tema»**: si al tema elegido le faltan iconos o cursor, Rexon los descarga
  en tu carpeta personal y los aplica. Si lo desactivas, primero te
  preguntará. Para los conjuntos que solo se instalan con paquetes del
  sistema, Rexon siempre pregunta.

Si la sesión no tiene bandeja, Rexon te lo dirá: no se pone el icono, y la
cruz cierra la aplicación como siempre.

## Gestor de archivos

Elige qué aplicación abre las carpetas, tanto en Rexon como en todo el
sistema. **«Automático (según el escritorio)»** usa el gestor propio del
escritorio actual. Junto a cada uno se ve si está instalado o no. Pulsa
**«Aplicar los ajustes del gestor de archivos»**. Hyprland y otros gestores
de ventanas no tienen gestor de archivos propio: sirve cualquiera de la
lista.

## Cómo recuperar las aplicaciones predeterminadas

Después de instalar otro escritorio, el terminal, el gestor de archivos y el
editor de texto pueden cambiar a los suyos.

![Restauración rápida de aplicaciones e integridad del escritorio](../images/settings-quick-restore.png)

**Rápido:** en la sección **🔧 Restauración rápida de aplicaciones**, pulsa
**«Restaurar las aplicaciones de …»** (ahí aparece el nombre de tu
escritorio, por ejemplo «Restaurar las aplicaciones de MATE»).

**Tus propias aplicaciones:** en la sección
**🔒 Protección de aplicaciones predeterminadas**:

1. Activa **«Activar la protección (recomendado)»**.
2. Marca con las casillas qué proteger: gestor de archivos, navegador,
   correo, visor de imágenes, vídeo, PDF, archivos comprimidos, música,
   terminal. Al lado se ve qué aplicación está guardada.
3. Pulsa **«📌 Recordar ahora»**: Rexon recordará tu elección actual.
4. Si aun así se han sustituido las aplicaciones, pulsa
   **«↩️ Restaurar como estaba»**.

![Protección de aplicaciones predeterminadas](../images/settings-protection.png)

## Colores del terminal después de cambiar de tema

Algunos terminales toman por defecto los colores del texto y del fondo del
tema. Con un tema oscuro poco acertado, el texto puede quedar gris oscuro
sobre negro. Por eso, después de cambiar el tema GTK, el tema global, el
esquema de color, el modo claro/oscuro, el color de acento o el aspecto «Como
en una distribución», Rexon da a esos terminales su propia pareja de colores
legible: texto claro sobre fondo oscuro para un tema oscuro, y oscuro sobre
claro para uno claro.

- **A qué terminales afecta:** mate-terminal, gnome-terminal y Tilix (que
  toman los colores del tema por defecto), y también xfce4-terminal y
  Terminator si tienen activados los colores del tema. Konsole, Console,
  Ptyxis, kitty, Alacritty y otros terminales con esquemas propios no se
  tocan.
- **Tus propios colores no se tocan.** Si has elegido tú los colores en el
  perfil y se leen bien, Rexon los deja.
- **Cómo desactivarlo:** en la sección
  **🔒 Protección de aplicaciones predeterminadas**, desmarca la casilla
  **«No dejar que los temas estropeen los colores del terminal
  (recomendado)»**. Está activada por defecto.
- **Cómo deshacerlo:** Rexon guarda los colores anteriores del terminal junto
  con el tema. El botón **«Restaurar como estaba antes de «…»»** de la
  pestaña «Apariencia» (o **«↩️ Restaurar como estaba»** en el aspecto «Como
  en una distribución») también los restaura.

## Copias del panel

**🛡️ Protección del panel** guarda el aspecto del panel, los applets y los
atajos de teclado de su escritorio y los recupera si algo se estropeó tras
instalar otro escritorio. Funciona en todos los escritorios que conoce Rexon.

1. Pulse **«📦 Guardar copia»**. La pista sobre el botón dice qué se copia en
   este escritorio.
2. Se guardan hasta 10 copias por escritorio; la primera de todas
   (**«⭐ la original, antes de Rexon»**) no se borra nunca.
3. Antes de instalar o cambiar de escritorio, Rexon copia por sí mismo los
   paneles de **todos los escritorios instalados**.
4. Si un panel se estropeó, pulse **«Restaurar»** junto a la copia que quiera.
   Las copias de otros escritorios están en el bloque plegado **«Copias de
   otros escritorios»**; ese escritorio la tomará al volver a iniciar sesión.

GNOME, Budgie y Pantheon comparten parte de los atajos: Rexon nombra en la
pregunta los escritorios que también cambiarán.

## Fondos de pantalla, sonidos y demás

- **🎬 Fondo de pantalla**: **«🖼️ Imagen…»** y **«🎬 Vídeo…»** ponen un
  fondo de pantalla desde un archivo (imágenes JPG, PNG, WebP; vídeos MP4,
  WebM, MKV y otros; Flash SWF). Debajo, un enlace de YouTube o Vimeo
  (**Calidad**, **Modo**: «En línea» o «Descargar») y fondos de pantalla web
  desde un enlace o un archivo HTML. Para YouTube hace falta la aplicación
  yt-dlp: si no está, Rexon te ofrecerá instalarla. **«⏹️ Detener los
  fondos de pantalla animados»** quita los fondos de pantalla animados.
- Ahí mismo está el sonido de los fondos de pantalla: volumen, «Silenciar
  cuando suene otro programa» y «Detener el fondo de pantalla cuando no se
  vea detrás de las ventanas». El sonido cambia al instante; no hace falta
  volver a poner el fondo de pantalla.

  ![La sección «Fondo de pantalla»](../images/settings-wallpaper.png)

- **🪟 Importar temas de Windows**: **«📥 Elegir un archivo de tema…»**:
  temas (.themepack, .theme), estilos visuales (.msstyles), iconos, cursores
  y fondos de pantalla de vídeo DreamScene. El tema listo aparecerá en la
  pestaña «Extensiones».
- **🧩 Integridad del escritorio**: **«Comprobar e instalar lo que falte»**
  comprueba si el escritorio tiene instalado todo lo que necesita (temas,
  tipos de letra, complementos) e instala lo que falte. El sistema te pedirá
  la contraseña.

## Si algo sale mal

- **El idioma no ha cambiado.** Pulsa **«Reiniciar Rexon»**: el idioma solo
  cambia después de reiniciar.
- **No hay icono junto al reloj.** Esta sesión no tiene bandeja del sistema.
  Rexon funciona como una ventana normal; «Iniciar al iniciar sesión» lo
  abrirá igualmente al iniciar sesión.
- **No se ha podido cambiar el inicio automático.** Tienes los detalles en el
  registro de Rexon.
- **Las aplicaciones predeterminadas han cambiado después de instalar un
  escritorio.** Pulsa **«Restaurar las aplicaciones de …»** o
  **«↩️ Restaurar como estaba»** en la protección de aplicaciones.
- **«⚠️ El motor de fondos de pantalla no se ha cargado: la sección no está
  disponible».** La sección de fondos de pantalla no funcionará hasta que se
  inicie el motor de fondos de pantalla; abre la pestaña Wallpaper Engine y
  mira qué indica.
- **Han desaparecido las animaciones o los tipos de letra.** Ahora están en
  la pestaña «Apariencia»: en la parte inferior de esta pestaña hay enlaces
  que llevan directamente allí.
