import React from 'react'
import 'style/footer.scss'

function Footer (props) {
  return (
    <footer>
      <div className='version'>
        <h2> {props.version} </h2>
      </div>
    </footer>
  )
}

export default Footer
